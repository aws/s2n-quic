// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    message::{Handle, Message},
    network::{Buffers, HostId},
};
use crate::{
    features::Gso,
    socket::{
        ring, stats, task,
        task::{rx, tx},
    },
    syscall::SocketEvents,
};
use core::task::{Context, Poll};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    path::MaxMtu,
};
use std::{fmt, io, sync::Arc};

/// A task to receive on a socket
pub async fn rx(
    socket: Socket,
    producer: ring::Producer<Message>,
    stats: stats::Sender,
) -> io::Result<()> {
    let result = task::Receiver::new(producer, socket, Default::default(), stats).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

/// A task to send on a socket
pub async fn tx(
    socket: Socket,
    consumer: ring::Consumer<Message>,
    gso: Gso,
    stats: stats::Sender,
) -> io::Result<()> {
    let result = task::Sender::new(consumer, socket, gso, Default::default(), stats).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

#[derive(Clone)]
pub struct Socket(Arc<State>);

impl fmt::Debug for Socket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut f = f.debug_struct("Socket");

        f.field("host", &self.0.host);

        if let Ok(addr) = self.local_addr() {
            f.field("local_addr", &addr);
        }

        let _ = self.0.buffers.tx_host(self.0.host, |queue| {
            f.field("tx_queue", &queue.len());
        });

        let _ = self.0.buffers.rx_host(self.0.host, |queue| {
            f.field("rx_queue", &queue.len());
        });

        f.finish()
    }
}

impl Socket {
    pub(super) fn new(buffers: Buffers, host: HostId) -> Self {
        Self(Arc::new(State { buffers, host }))
    }

    /// Returns the current local address
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.0.buffers.lookup_addr(self.0.host)
    }

    /// Rebinds the address to a new address
    pub fn rebind(&self, addr: std::net::SocketAddr) {
        self.0.buffers.rebind(self.0.host, addr);
    }

    /// Sends a packet to the provided destination
    pub fn send_to(
        &self,
        addr: std::net::SocketAddr,
        ecn: ExplicitCongestionNotification,
        payload: Vec<u8>,
    ) -> std::io::Result<()> {
        self.0.buffers.tx_host(self.0.host, |queue| {
            let path = Handle {
                local_address: Default::default(),
                remote_address: SocketAddress::from(addr).into(),
            };
            let packet = super::network::Packet { path, ecn, payload };
            queue.send_packet(packet);
        })?;

        Ok(())
    }

    /// Receives a packet from a peer
    pub async fn recv_from(
        &self,
    ) -> std::io::Result<(
        std::net::SocketAddr,
        ExplicitCongestionNotification,
        Vec<u8>,
    )> {
        futures::future::poll_fn(|cx| self.poll_recv_from(cx)).await
    }

    pub fn try_recv_from(
        &self,
    ) -> std::io::Result<
        Option<(
            std::net::SocketAddr,
            ExplicitCongestionNotification,
            Vec<u8>,
        )>,
    > {
        let mut packet = Poll::Pending;

        self.0
            .buffers
            .rx_host(self.0.host, |queue| packet = queue.recv_packet(None))?;

        match packet {
            Poll::Ready(packet) => {
                let super::network::Packet { path, ecn, payload } = packet;
                let remote_address = path.remote_address.0.into();
                Ok(Some((remote_address, ecn, payload)))
            }
            Poll::Pending => Ok(None),
        }
    }

    pub fn poll_recv_from(
        &self,
        cx: &mut Context,
    ) -> Poll<
        std::io::Result<(
            std::net::SocketAddr,
            ExplicitCongestionNotification,
            Vec<u8>,
        )>,
    > {
        let mut packet = Poll::Pending;

        if let Err(err) = self
            .0
            .buffers
            .rx_host(self.0.host, |queue| packet = queue.recv_packet(Some(cx)))
        {
            return Err(err).into();
        }

        match packet {
            Poll::Ready(packet) => {
                let super::network::Packet { path, ecn, payload } = packet;
                let remote_address = path.remote_address.0.into();
                Ok((remote_address, ecn, payload)).into()
            }
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn rx_task(
        &self,
        max_mtu: MaxMtu,
        queue_recv_buffer_size: Option<u32>,
        stats: stats::Sender,
    ) -> impl s2n_quic_core::io::rx::Rx<PathHandle = Handle> {
        let payload_len = {
            let max_mtu: u16 = max_mtu.into();
            max_mtu as u32
        };

        let rx_buffer_size = queue_recv_buffer_size.unwrap_or(8u32 * (1 << 20));
        let entries = rx_buffer_size / payload_len;
        let entries = if entries.is_power_of_two() {
            entries
        } else {
            // round up to the nearest power of two, since the ring buffers require it
            entries.next_power_of_two()
        };

        let mut consumers = vec![];

        let (producer, consumer) = crate::socket::ring::pair(entries, payload_len);
        consumers.push(consumer);

        // spawn a task that actually reads from the socket into the ring buffer
        super::spawn(super::socket::rx(self.clone(), producer, stats));

        // construct the RX side for the endpoint event loop
        let max_mtu = MaxMtu::try_from(payload_len as u16).unwrap();
        let handle = self.local_addr().unwrap();
        let handle = SocketAddress::from(handle);
        crate::socket::io::rx::Rx::new(consumers, max_mtu, handle.into())
    }

    pub fn tx_task(
        &self,
        max_mtu: MaxMtu,
        queue_send_buffer_size: Option<u32>,
        stats: stats::Sender,
    ) -> impl s2n_quic_core::io::tx::Tx<PathHandle = Handle> {
        let gso = crate::features::Gso::default();
        gso.disable();

        // compute the payload size for each message from the number of GSO segments we can
        // fill
        let payload_len = {
            let max_mtu: u16 = max_mtu.into();
            (max_mtu as u32 * gso.max_segments() as u32).min(u16::MAX as u32)
        };

        let tx_buffer_size = queue_send_buffer_size.unwrap_or(128 * 1024);
        let entries = tx_buffer_size / payload_len;
        let entries = if entries.is_power_of_two() {
            entries
        } else {
            // round up to the nearest power of two, since the ring buffers require it
            entries.next_power_of_two()
        };

        let mut producers = vec![];

        let (producer, consumer) = crate::socket::ring::pair(entries, payload_len);
        producers.push(producer);

        // spawn a task that actually flushes the ring buffer to the socket
        super::spawn(super::socket::tx(
            self.clone(),
            consumer,
            gso.clone(),
            stats,
        ));

        // construct the TX side for the endpoint event loop
        crate::socket::io::tx::Tx::new(producers, gso, max_mtu)
    }
}

struct State {
    host: HostId,
    buffers: Buffers,
}

impl Drop for State {
    fn drop(&mut self) {
        self.buffers.close_host(self.host);
    }
}

impl tx::Socket<Message> for Socket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        _cx: &mut Context,
        entries: &mut [Message],
        events: &mut tx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        let mut count = 0;

        let res = self.0.buffers.tx_host(self.0.host, |queue| {
            count = queue.send(entries);
            let _ = events.on_complete(count);
        });

        if count > 0 {
            stats.send().on_operation_result(&res, |_| count);
        } else {
            stats.send().on_operation_pending();
        }

        res
    }
}

impl rx::Socket<Message> for Socket {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut rx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        let mut count = 0;

        let res = self.0.buffers.rx_host(self.0.host, |queue| {
            count = queue.recv(cx, entries);
            if count > 0 {
                let _ = events.on_complete(count);
            } else {
                events.blocked()
            }
        });

        if count > 0 {
            stats.recv().on_operation_result(&res, |_| count);
        } else {
            stats.recv().on_operation_pending();
        }

        res
    }
}
