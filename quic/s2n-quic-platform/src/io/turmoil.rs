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
    path::{self, MaxMtu},
};
use std::{convert::TryInto, io, io::ErrorKind};
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

        let local_addr = socket.local_addr()?;
        let local_addr: inet::SocketAddress = local_addr.into();
        let payload_len: usize = max_mtu.into();
        let payload_len = payload_len as u32;

        let (rx, rx_producer) = {
            let entries = 1024;

            let mut consumers = vec![];

            let (producer, consumer) = ring::pair(entries, payload_len);
            consumers.push(consumer);

            let rx = rx::Rx::new(consumers, max_mtu, local_addr.into());

            (rx, producer)
        };

        let (tx, tx_consumer) = {
            let entries = 1024;

            let mut producers = vec![];

            let (producer, consumer) = ring::pair(entries, payload_len);
            producers.push(producer);

            let gso = crate::features::Gso::default();
            gso.disable();
            let tx = tx::Tx::new(producers, gso, max_mtu);

            (tx, consumer)
        };

        tokio::spawn(run_io(socket, rx_producer, tx_consumer));

        let el = EventLoop {
            clock,
            rx,
            tx,
            endpoint,
        }
        .start();

        Ok((el, local_addr))
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
        let consumer_ready = poll_fn(|cx| consumer.poll_acquire(1, cx));
        let producer_ready = async {
            if poll_producer {
                poll_fn(|cx| producer.poll_acquire(1, cx)).await
            } else {
                core::future::pending().await
            }
        };

        let is_readable = Select::new(
            consumer_ready,
            producer_ready,
            core::future::pending(),
            socket_ready,
        )
        .await
        .unwrap()
        .timeout_expired;

        if is_readable {
            let mut count = 0;
            for entry in producer.data() {
                if let Ok((len, addr)) = socket.try_recv_from(entry.payload_mut()) {
                    count += 1;
                    entry.set_remote_address(&(addr.into()));
                    unsafe {
                        entry.set_payload_len(len);
                    }
                } else {
                    break;
                }
            }

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
                if socket.try_send_to(payload, addr).is_ok() {
                    count += 1;
                } else {
                    break;
                }
            }
            consumer.release(count);
        }

        if !(producer.is_open() || consumer.is_open()) {
            return Ok(());
        }
    }
}
