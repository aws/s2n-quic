// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, io::tokio::Clock, socket::std as socket};
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::SocketAddress,
    io::event_loop::select::{self, Select},
    path::MaxMtu,
    time::{
        clock::{ClockWithTimer as _, Timer as _},
        Clock as ClockTrait,
    },
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::runtime::Handle;
use turmoil::net::UdpSocket;

mod builder;
#[cfg(test)]
mod tests;

pub use builder::Builder;

impl crate::socket::std::Socket for UdpSocket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error> {
        let (len, addr) = self.try_recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error> {
        let addr: std::net::SocketAddr = (*addr).into();
        self.try_send_to(buf, addr)
    }
}

pub type PathHandle = socket::Handle;

#[derive(Default)]
pub struct Io {
    builder: Builder,
}

impl Io {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn new<A: turmoil::ToSocketAddrs + Send + Sync + 'static>(addr: A) -> io::Result<Self> {
        let builder = Builder::default().with_address(addr)?;
        Ok(Self { builder })
    }

    async fn setup<E: Endpoint<PathHandle = PathHandle>>(
        self,
        mut endpoint: E,
    ) -> io::Result<(Instance<E>, SocketAddress)> {
        let Builder {
            handle: _,
            socket,
            addr,
            max_mtu,
        } = self.builder;

        endpoint.set_max_mtu(max_mtu);

        let clock = Clock::default();

        let socket = if let Some(socket) = socket {
            socket
        } else if let Some(addr) = addr {
            UdpSocket::bind(&*addr).await?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing bind address",
            ));
        };

        let rx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let mut rx = socket::Queue::new(rx_buffer);

        let tx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let tx = socket::Queue::new(tx_buffer);

        let local_addr: SocketAddress = socket.local_addr()?.into();

        // tell the queue the local address so it can fill it in on each message
        rx.set_local_address(local_addr.into());

        let instance = Instance {
            clock,
            socket,
            rx,
            tx,
            endpoint,
        };

        Ok((instance, local_addr))
    }

    pub fn start<E: Endpoint<PathHandle = PathHandle>>(
        mut self,
        endpoint: E,
    ) -> io::Result<(tokio::task::JoinHandle<()>, SocketAddress)> {
        let handle = if let Some(handle) = self.builder.handle.take() {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let task = handle.spawn(async move {
            let (instance, _local_addr) = self.setup(endpoint).await.unwrap();

            if let Err(err) = instance.event_loop().await {
                let debug = format!("A fatal IO error occurred ({:?}): {err}", err.kind());
                if cfg!(test) {
                    panic!("{debug}");
                } else {
                    eprintln!("{debug}");
                }
            }
        });

        drop(guard);

        // TODO this is a potentially async operation - can we get this here?
        let local_addr = Default::default();

        Ok((task, local_addr))
    }
}

struct Instance<E> {
    clock: Clock,
    socket: turmoil::net::UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint<PathHandle = PathHandle>> Instance<E> {
    async fn event_loop(self) -> io::Result<()> {
        let Self {
            clock,
            socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        let mut timer = clock.timer();

        loop {
            // Poll for readability if we have free slots available
            let rx_interest = rx.free_len() > 0;
            let rx_task = async {
                if rx_interest {
                    socket.readable().await
                } else {
                    futures::future::pending().await
                }
            };

            // Poll for writablity if we have occupied slots available
            let tx_interest = tx.occupied_len() > 0;
            let tx_task = async {
                if tx_interest {
                    socket.writable().await
                } else {
                    futures::future::pending().await
                }
            };

            let wakeups = endpoint.wakeups(&clock);
            // pin the wakeups future so we don't have to move it into the Select future.
            tokio::pin!(wakeups);

            let timer_ready = timer.ready();

            let select::Outcome {
                rx_result,
                tx_result,
                timeout_expired,
                application_wakeup,
            } = if let Ok(res) = Select::new(rx_task, tx_task, &mut wakeups, timer_ready).await {
                res
            } else {
                // The endpoint has shut down
                return Ok(());
            };

            let wakeup_timestamp = clock.get_time();
            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp: wakeup_timestamp,
                },
                None,
                subscriber,
            );

            publisher.on_platform_event_loop_wakeup(event::builder::PlatformEventLoopWakeup {
                timeout_expired,
                rx_ready: rx_result.is_some(),
                tx_ready: tx_result.is_some(),
                application_wakeup,
            });

            if tx_result.is_some() {
                tx.tx(&socket, &mut publisher)?;
            }

            if rx_result.is_some() {
                rx.rx(&socket, &mut publisher)?;
                endpoint.receive(&mut rx.rx_queue(), &clock);
            }

            endpoint.transmit(&mut tx.tx_queue(), &clock);

            let timeout = endpoint.timeout();

            if let Some(timeout) = timeout {
                timer.update(timeout);
            }

            let timestamp = clock.get_time();
            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp,
                },
                None,
                subscriber,
            );

            // notify the application that we're going to sleep
            let timeout = timeout.map(|t| t.saturating_duration_since(timestamp));
            publisher.on_platform_event_loop_sleep(event::builder::PlatformEventLoopSleep {
                timeout,
                processing_duration: timestamp.saturating_duration_since(wakeup_timestamp),
            });
        }
    }
}
