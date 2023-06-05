// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, features::gso, socket::default as socket, syscall};
use cfg_if::cfg_if;
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::{self, SocketAddress},
    io::event_loop::select::{self, Select},
    path::MaxMtu,
    time::{
        clock::{ClockWithTimer as _, Timer as _},
        Clock as ClockTrait,
    },
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::{net::UdpSocket, runtime::Handle};

mod builder;
mod clock;
mod task;
#[cfg(test)]
mod tests;

pub type PathHandle = socket::Handle;
pub use builder::Builder;
pub(crate) use clock::Clock;

impl crate::socket::std::Socket for UdpSocket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error> {
        let (len, addr) = self.try_recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error> {
        self.try_send_to(buf, (*addr).into())
    }
}

#[derive(Debug, Default)]
pub struct Io {
    builder: Builder,
}

impl Io {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn new<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let address = addr.to_socket_addrs()?.next().expect("missing address");
        let builder = Builder::default().with_receive_address(address)?;
        Ok(Self { builder })
    }

    pub fn start<E: Endpoint<PathHandle = PathHandle>>(
        self,
        mut endpoint: E,
    ) -> io::Result<(tokio::task::JoinHandle<()>, SocketAddress)> {
        let Builder {
            handle,
            rx_socket,
            tx_socket,
            recv_addr,
            send_addr,
            recv_buffer_size,
            send_buffer_size,
            mut max_mtu,
            max_segments,
            reuse_port,
        } = self.builder;

        let clock = Clock::default();

        let mut publisher = event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                endpoint_type: E::ENDPOINT_TYPE,
                timestamp: clock.get_time(),
            },
            None,
            endpoint.subscriber(),
        );

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Gso {
                max_segments: max_segments.into(),
            },
        });

        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let rx_socket = if let Some(rx_socket) = rx_socket {
            rx_socket
        } else if let Some(recv_addr) = recv_addr {
            syscall::bind_udp(recv_addr, reuse_port)?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing bind address",
            ));
        };

        // ensure the socket is non-blocking
        rx_socket.set_nonblocking(true)?;

        let tx_socket = if let Some(tx_socket) = tx_socket {
            tx_socket
        } else if let Some(send_addr) = send_addr {
            syscall::bind_udp(send_addr, reuse_port)?
        } else {
            // No tx_socket or send address was specified, so the tx socket
            // will be a handle to the rx socket.
            rx_socket.try_clone()?
        };

        // ensure the socket is non-blocking
        tx_socket.set_nonblocking(true)?;

        if let Some(size) = send_buffer_size {
            tx_socket.set_send_buffer_size(size)?;
        }

        if let Some(size) = recv_buffer_size {
            rx_socket.set_recv_buffer_size(size)?;
        }

        fn convert_addr_to_std(addr: socket2::SockAddr) -> io::Result<std::net::SocketAddr> {
            addr.as_socket().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid domain for socket")
            })
        }

        #[allow(unused_variables)] // some platform builds won't use these so ignore warnings
        let (tx_addr, rx_addr) = (
            convert_addr_to_std(tx_socket.local_addr()?)?,
            convert_addr_to_std(rx_socket.local_addr()?)?,
        );

        // Configure MTU discovery
        if !syscall::configure_mtu_disc(&tx_socket) {
            // disable MTU probing if we can't prevent fragmentation
            max_mtu = MaxMtu::MIN;
        }

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::MaxMtu {
                mtu: max_mtu.into(),
            },
        });

        // Configure packet info CMSG
        syscall::configure_pktinfo(&rx_socket);

        // Configure TOS/ECN
        let tos_enabled = syscall::configure_tos(&rx_socket);

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Ecn {
                enabled: tos_enabled,
            },
        });

        let rx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let tx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let mut rx = socket::Queue::<buffer::Buffer>::new(rx_buffer, max_segments.into());
                let tx = socket::Queue::<buffer::Buffer>::new(tx_buffer, max_segments.into());
            } else {
                // If you are using an LSP to jump into this code, it will
                // probably take you to the wrong implementation. socket.rs does
                // compile time swaps of socket implementations. This queue is
                // actually in socket/std.rs, not socket/mmsg.rs
                let mut rx = socket::Queue::new(rx_buffer);
                let tx = socket::Queue::new(tx_buffer);
            }
        }

        // tell the queue the local address so it can fill it in on each message
        rx.set_local_address({
            let addr: inet::SocketAddress = rx_addr.into();
            addr.into()
        });

        // Notify the endpoint of the MTU that we chose
        endpoint.set_max_mtu(max_mtu);

        let instance = Instance {
            clock,
            rx_socket: rx_socket.into(),
            tx_socket: tx_socket.into(),
            rx,
            tx,
            endpoint,
        };

        let local_addr = instance.rx_socket.local_addr()?.into();

        let task = handle.spawn(async move {
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

        Ok((task, local_addr))
    }
}

#[derive(Debug)]
struct Instance<E> {
    clock: Clock,
    rx_socket: std::net::UdpSocket,
    tx_socket: std::net::UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint<PathHandle = PathHandle>> Instance<E> {
    async fn event_loop(self) -> io::Result<()> {
        let Self {
            clock,
            rx_socket,
            tx_socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let rx_socket = tokio::io::unix::AsyncFd::new(rx_socket)?;
                let tx_socket = tokio::io::unix::AsyncFd::new(tx_socket)?;
            } else {
                let rx_socket = async_fd_shim::AsyncFd::new(rx_socket)?;
                let tx_socket = async_fd_shim::AsyncFd::new(tx_socket)?;
            }
        }

        let mut timer = clock.timer();

        loop {
            // Poll for readability if we have free slots available
            let rx_interest = rx.free_len() > 0;
            let rx_task = async {
                if rx_interest {
                    rx_socket.readable().await
                } else {
                    futures::future::pending().await
                }
            };

            // Poll for writablity if we have occupied slots available
            let tx_interest = tx.occupied_len() > 0;
            let tx_task = async {
                if tx_interest {
                    tx_socket.writable().await
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

            if let Some(guard) = tx_result {
                if let Ok(result) = guard?.try_io(|socket| tx.tx(socket, &mut publisher)) {
                    result?;
                }
            }

            if let Some(guard) = rx_result {
                if let Ok(result) = guard?.try_io(|socket| rx.rx(socket, &mut publisher)) {
                    result?;
                }
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

/// A shim for the AsyncFd API
///
/// Tokio only provides the AsyncFd interface for unix platforms so for
/// other platforms that don't support the msg/mmsg APIs we use a small
/// shim to reduce the differences between each implementation.
mod async_fd_shim {
    #![allow(dead_code)]

    use super::*;

    pub struct AsyncFd(tokio::net::UdpSocket);

    impl AsyncFd {
        pub fn new(socket: std::net::UdpSocket) -> io::Result<Self> {
            let socket = tokio::net::UdpSocket::from_std(socket)?;
            Ok(Self(socket))
        }

        pub async fn readable(&self) -> io::Result<TryIo<'_>> {
            self.0.readable().await?;
            Ok(TryIo(&self.0))
        }

        pub async fn writable(&self) -> io::Result<TryIo<'_>> {
            self.0.writable().await?;
            Ok(TryIo(&self.0))
        }
    }

    pub struct TryIo<'a>(&'a tokio::net::UdpSocket);

    impl<'a> TryIo<'a> {
        pub fn try_io<R>(
            &mut self,
            f: impl FnOnce(&tokio::net::UdpSocket) -> io::Result<R>,
        ) -> Result<io::Result<R>, ()> {
            match f(self.0) {
                Ok(v) => Ok(Ok(v)),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => Err(()),
                Err(err) => Ok(Err(err)),
            }
        }
    }
}
