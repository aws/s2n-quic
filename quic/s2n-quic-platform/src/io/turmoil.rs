// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    io::tokio::Clock,
    message::{simple::Message, Message as _},
    socket::{
        io::{rx, tx},
        ring::{self, Consumer, Producer},
    },
};
use core::future::Future;
use futures::future::poll_fn;
use s2n_quic_core::{
    endpoint::Endpoint,
    inet::{self, SocketAddress},
    io::event_loop::{select::Select, EventLoop},
    path::{self, mtu},
};
use std::{io, io::ErrorKind};
use tokio::runtime::Handle;
use turmoil::net::UdpSocket;

mod builder;
#[cfg(test)]
mod tests;

pub use builder::Builder;
pub type PathHandle = path::Tuple;

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
    ) -> io::Result<(impl Future<Output = ()>, SocketAddress)> {
        let Builder {
            handle: _,
            socket,
            addr,
            mtu_config_builder,
        } = self.builder;

        let mtu_config = mtu_config_builder
            .build()
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?;

        endpoint.set_mtu_config(mtu_config);

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

        let local_addr = socket.local_addr()?;
        let local_addr: inet::SocketAddress = local_addr.into();
        let payload_len: usize = mtu_config.max_mtu.into();
        let payload_len = payload_len as u32;

        // This number is somewhat arbitrary but it's a decent number of messages without it consuming
        // large in memory. Eventually, it might be a good idea to expose this value in the
        // builder, but we'll wait until someone asks for it :).
        let entries = 1024;

        let (rx, rx_producer) = {
            let mut consumers = vec![];

            let (producer, consumer) = ring::pair(entries, payload_len);
            consumers.push(consumer);

            let rx = rx::Rx::new(consumers, mtu_config.max_mtu, local_addr.into());

            (rx, producer)
        };

        let (tx, tx_consumer) = {
            let mut producers = vec![];

            let (producer, consumer) = ring::pair(entries, payload_len);
            producers.push(producer);

            let gso = crate::features::Gso::default();

            // GSO is not supported by turmoil so disable it
            gso.disable();

            let tx = tx::Tx::new(producers, gso, mtu_config.max_mtu);

            (tx, consumer)
        };

        // Spawn a task that does the actual socket calls and coordinates with the event loop
        // through the ring buffers
        tokio::spawn(run_io(socket, rx_producer, tx_consumer));

        let event_loop = EventLoop {
            clock,
            rx,
            tx,
            endpoint,
            cooldown: Default::default(),
        }
        .start();

        Ok((event_loop, local_addr))
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

            instance.await;
        });

        drop(guard);

        // TODO this is a potentially async operation - can we get this here?
        let local_addr = Default::default();

        Ok((task, local_addr))
    }
}

/// Turmoil doesn't allow to split sockets for Tx and Rx so we need to spawn a single task to
/// handle both jobs
async fn run_io(
    socket: UdpSocket,
    mut producer: Producer<Message>,
    mut consumer: Consumer<Message>,
) -> io::Result<()> {
    let mut poll_producer = false;

    loop {
        let socket_ready = socket.readable();
        let consumer_ready = poll_fn(|cx| consumer.poll_acquire(u32::MAX, cx));
        let producer_ready = async {
            // Only poll the producer if we need more capacity - otherwise we would constantly wake
            // up
            if poll_producer {
                poll_fn(|cx| producer.poll_acquire(u32::MAX, cx)).await
            } else {
                core::future::pending().await
            }
        };
        // The socket task doesn't have any application wakeups to handle so just make it pending
        let application_wakeup = core::future::pending();

        // We replace the timer future with the `socket_ready` instead, since we don't have a
        // timer here. Other than the application wakeup, Select doesn't really treat any of
        // the futures special.
        let is_readable = Select::new(
            consumer_ready,
            producer_ready,
            application_wakeup,
            socket_ready,
        )
        .await
        .unwrap()
        .timeout_expired;

        if is_readable {
            let mut count = 0;
            for entry in producer.data() {
                // Since UDP sockets are stateless, the only errors we should back is a WouldBlock.
                // If we get any errors, we'll try again later.
                if let Ok((len, addr)) = socket.try_recv_from(entry.payload_mut()) {
                    count += 1;
                    // update the packet information
                    entry.set_remote_address(&(addr.into()));
                    unsafe {
                        entry.set_payload_len(len);
                    }
                } else {
                    break;
                }
            }

            // release the received messages to the consumer
            producer.release(count);

            // only poll the producer if we need entries
            poll_producer = producer.data().is_empty();
        }

        {
            let mut count = 0;
            for entry in consumer.data() {
                let addr = *entry.remote_address();
                let addr: std::net::SocketAddr = addr.into();
                let payload = entry.payload_mut();
                // Since UDP sockets are stateless, the only errors we should back is a WouldBlock.
                // If we get any errors, we'll try again later.
                if socket.try_send_to(payload, addr).is_ok() {
                    count += 1;
                } else {
                    break;
                }
            }

            // release capacity back to the producer
            consumer.release(count);
        }

        // check to see if the rings are open, otherwise we need to shut down the task
        if !(producer.is_open() && consumer.is_open()) {
            return Ok(());
        }
    }
}
