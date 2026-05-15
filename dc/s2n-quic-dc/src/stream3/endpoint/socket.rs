// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::socket::{BusyPoll, Gso as GsoSocket, Options};
use s2n_quic_platform::features;
use std::{io, net::SocketAddr};

const DEFAULT_BUFFER_SIZE: usize = 200 * 1024 * 1024;

/// Configuration for send socket creation.
pub struct SendConfig {
    pub num_sockets: usize,
    pub bind_addr: SocketAddr,
    pub gso: features::Gso,
    pub send_buffer: usize,
}

impl SendConfig {
    pub fn new(num_sockets: usize, bind_addr: SocketAddr, gso: features::Gso) -> Self {
        Self {
            num_sockets,
            bind_addr,
            gso,
            send_buffer: DEFAULT_BUFFER_SIZE,
        }
    }

    /// Creates send sockets with GSO support.
    ///
    /// Each socket binds to an ephemeral port on the given address. Recv buffer is zeroed
    /// since these sockets don't receive.
    pub fn create(&self) -> io::Result<Vec<GsoSocket<std::net::UdpSocket>>> {
        let mut sockets = Vec::with_capacity(self.num_sockets);

        let mut bind_addr = self.bind_addr;
        bind_addr.set_port(0);

        for _ in 0..self.num_sockets {
            let mut opts = Options::default();
            opts.addr = bind_addr;
            opts.blocking = false;
            opts.send_buffer = Some(self.send_buffer);
            opts.recv_buffer = Some(0);
            let socket = opts.build_udp()?;

            let socket = GsoSocket(socket, self.gso.clone());
            sockets.push(socket);
        }

        Ok(sockets)
    }

    pub fn busy_poll(&self) -> io::Result<Vec<GsoSocket<BusyPoll<std::net::UdpSocket>>>> {
        let sockets = self.create()?;
        Ok(sockets
            .into_iter()
            .map(|GsoSocket(s, gso)| GsoSocket(BusyPoll(s), gso))
            .collect())
    }
}

/// Configuration for receive socket creation.
///
/// Each recv socket binds to its own distinct address so that remote senders can
/// target individual recv workers directly (bypassing kernel RSS). The full list
/// of bound addresses is advertised to peers during the handshake.
pub struct RecvConfig {
    pub num_sockets: usize,
    pub bind_addr: SocketAddr,
    pub recv_buffer: usize,
}

impl RecvConfig {
    pub fn new(num_sockets: usize, bind_addr: SocketAddr) -> Self {
        Self {
            num_sockets,
            bind_addr,
            recv_buffer: DEFAULT_BUFFER_SIZE,
        }
    }

    /// Creates receive sockets, each bound to its own ephemeral port.
    ///
    /// Every socket gets a distinct address on the same IP so remote senders can
    /// distribute traffic across recv workers without relying on RSS hashing.
    /// GRO is enabled for coalescing received segments. Send buffer is zeroed
    /// since these sockets don't send.
    pub fn create(&self) -> io::Result<Vec<std::net::UdpSocket>> {
        let mut sockets = Vec::with_capacity(self.num_sockets);

        let mut bind_addr = self.bind_addr;
        bind_addr.set_port(0);

        for _ in 0..self.num_sockets {
            let mut opts = Options::default();
            opts.addr = bind_addr;
            opts.gro = true;
            opts.blocking = false;
            opts.recv_buffer = Some(self.recv_buffer);
            opts.send_buffer = Some(0);
            sockets.push(opts.build_udp()?);
        }

        Ok(sockets)
    }

    pub fn busy_poll(&self) -> io::Result<Vec<BusyPoll<std::net::UdpSocket>>> {
        let sockets = self.create()?;
        Ok(sockets.into_iter().map(BusyPoll).collect())
    }
}

/// Wraps a send socket to count calls and bytes at the I/O boundary.
pub(crate) struct MeteredSend<S> {
    inner: S,
    tx_counter: crate::counter::Counter,
    tx_bytes_counter: crate::counter::Counter,
}

impl<S> MeteredSend<S> {
    pub fn new(
        inner: S,
        tx_counter: crate::counter::Counter,
        tx_bytes_counter: crate::counter::Counter,
    ) -> Self {
        Self {
            inner,
            tx_counter,
            tx_bytes_counter,
        }
    }
}

impl<S: crate::socket::send::Socket> crate::socket::send::Socket for MeteredSend<S> {
    #[inline]
    fn send_msg(
        &self,
        addr: &crate::msg::addr::Addr,
        payload: &[io::IoSlice],
        segment_size: u16,
        ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        let result = self.inner.send_msg(addr, payload, segment_size, ecn);
        if let Ok(sent) = &result {
            self.tx_counter.add(1);
            self.tx_bytes_counter.add(*sent as u64);
        }
        result
    }

    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

/// Wraps a recv socket to count calls and bytes at the I/O boundary.
pub(crate) struct MeteredRecv<S> {
    inner: S,
    rx_counter: crate::counter::Counter,
    rx_bytes_counter: crate::counter::Counter,
    rx_counter_total: crate::counter::Counter,
    rx_bytes_counter_total: crate::counter::Counter,
}

impl<S> MeteredRecv<S> {
    pub fn new(
        inner: S,
        rx_counter: crate::counter::Counter,
        rx_bytes_counter: crate::counter::Counter,
        rx_counter_total: crate::counter::Counter,
        rx_bytes_counter_total: crate::counter::Counter,
    ) -> Self {
        Self {
            inner,
            rx_counter,
            rx_bytes_counter,
            rx_counter_total,
            rx_bytes_counter_total,
        }
    }
}

impl<S: crate::socket::recv::Socket> crate::socket::recv::Socket for MeteredRecv<S> {
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut core::task::Context,
        addr: &mut crate::msg::addr::Addr,
        cmsg: &mut crate::msg::cmsg::Receiver,
        buffer: &mut [io::IoSliceMut],
    ) -> core::task::Poll<io::Result<usize>> {
        let result = self.inner.poll_recv(cx, addr, cmsg, buffer);
        if let core::task::Poll::Ready(Ok(received)) = &result {
            self.rx_counter.add(1);
            self.rx_bytes_counter.add(*received as u64);
            self.rx_counter_total.add(1);
            self.rx_bytes_counter_total.add(*received as u64);
        }
        result
    }

    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}
