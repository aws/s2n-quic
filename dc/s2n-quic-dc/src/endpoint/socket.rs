// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::socket::{BusyPoll, Gso as GsoSocket, Options};
use s2n_quic_platform::features;
use std::{ffi::CString, io, net::SocketAddr};

const DEFAULT_BUFFER_SIZE: usize = 200 * 1024 * 1024;

/// Per-socket bind configuration.
#[derive(Clone, Debug)]
pub struct BindAddress {
    pub addr: SocketAddr,
    pub ifname: Option<CString>,
}

impl From<SocketAddr> for BindAddress {
    #[inline]
    fn from(addr: SocketAddr) -> Self {
        Self { addr, ifname: None }
    }
}

/// Configuration for endpoint socket creation.
pub struct Config {
    pub bind_addrs: Vec<BindAddress>,
    pub num_send_sockets: usize,
    pub num_recv_sockets: usize,
    pub gso: features::Gso,
    pub send_buffer: usize,
    pub recv_buffer: usize,
}

impl Config {
    pub fn new(
        bind_addrs: Vec<BindAddress>,
        num_send_sockets: usize,
        num_recv_sockets: usize,
        gso: features::Gso,
    ) -> Self {
        Self {
            bind_addrs,
            num_send_sockets,
            num_recv_sockets,
            gso,
            send_buffer: DEFAULT_BUFFER_SIZE,
            recv_buffer: DEFAULT_BUFFER_SIZE,
        }
    }

    fn validate(&self) -> io::Result<()> {
        if self.bind_addrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at least one bind address is required",
            ));
        }

        let max_sockets = self.num_send_sockets.max(self.num_recv_sockets);
        if max_sockets == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at least one send or recv socket is required",
            ));
        }

        if self.bind_addrs.len() < max_sockets {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bind_addrs length must be >= max(num_send_sockets, num_recv_sockets)",
            ));
        }

        Ok(())
    }

    /// Creates send and receive sockets.
    pub fn create(
        &self,
    ) -> io::Result<(
        Vec<GsoSocket<std::net::UdpSocket>>,
        Vec<std::net::UdpSocket>,
    )> {
        self.validate()?;

        let shared_sockets = self.num_send_sockets.min(self.num_recv_sockets);
        let mut recv_sockets = Vec::with_capacity(self.num_recv_sockets);
        let mut send_sockets = Vec::with_capacity(self.num_send_sockets);

        for bind_addr in self.bind_addrs.iter().take(shared_sockets) {
            let mut opts = Options::default();
            opts.addr = bind_addr.addr;
            opts.bind_interface = bind_addr.ifname.clone();
            opts.gro = true;
            opts.blocking = false;
            opts.recv_buffer = Some(self.recv_buffer);
            opts.send_buffer = Some(self.send_buffer);
            let recv_socket = opts.build_udp()?;
            let send_socket = recv_socket.try_clone()?;

            recv_sockets.push(recv_socket);
            send_sockets.push(GsoSocket(send_socket, self.gso.clone()));
        }

        for bind_addr in self
            .bind_addrs
            .iter()
            .take(self.num_recv_sockets)
            .skip(shared_sockets)
        {
            let mut opts = Options::default();
            opts.addr = bind_addr.addr;
            opts.bind_interface = bind_addr.ifname.clone();
            opts.gro = true;
            opts.blocking = false;
            opts.recv_buffer = Some(self.recv_buffer);
            opts.send_buffer = Some(0);
            recv_sockets.push(opts.build_udp()?);
        }

        for bind_addr in self
            .bind_addrs
            .iter()
            .take(self.num_send_sockets)
            .skip(shared_sockets)
        {
            let mut opts = Options::default();
            opts.addr = bind_addr.addr;
            opts.bind_interface = bind_addr.ifname.clone();
            opts.blocking = false;
            opts.send_buffer = Some(self.send_buffer);
            opts.recv_buffer = Some(0);
            let socket = opts.build_udp()?;
            let socket = GsoSocket(socket, self.gso.clone());
            send_sockets.push(socket);
        }

        Ok((send_sockets, recv_sockets))
    }

    pub fn busy_poll(
        &self,
    ) -> io::Result<(
        Vec<GsoSocket<BusyPoll<std::net::UdpSocket>>>,
        Vec<BusyPoll<std::net::UdpSocket>>,
    )> {
        let (send_sockets, recv_sockets) = self.create()?;
        let send_sockets = send_sockets
            .into_iter()
            .map(|GsoSocket(s, gso)| GsoSocket(BusyPoll(s), gso))
            .collect();
        let recv_sockets = recv_sockets.into_iter().map(BusyPoll).collect();
        Ok((send_sockets, recv_sockets))
    }
}

/// Wraps a socket to count ops, bytes, and errors at the I/O boundary.
pub(crate) struct Metered<S> {
    inner: S,
    ops: crate::counter::Counter,
    bytes: crate::counter::Counter,
    errors: crate::counter::Counter,
}

impl<S: std::fmt::Debug> std::fmt::Debug for Metered<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<S> Metered<S> {
    pub fn new(
        inner: S,
        ops: crate::counter::Counter,
        bytes: crate::counter::Counter,
        errors: crate::counter::Counter,
    ) -> Self {
        Self {
            inner,
            ops,
            bytes,
            errors,
        }
    }
}

impl<S: crate::socket::LocalAddr> crate::socket::LocalAddr for Metered<S> {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}

impl<S: crate::socket::send::Socket> crate::socket::send::Socket for Metered<S> {
    #[inline]
    fn send_msg(
        &self,
        addr: &crate::msg::addr::Addr,
        payload: &[io::IoSlice],
        segment_size: u16,
        ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        let result = self.inner.send_msg(addr, payload, segment_size, ecn);
        match &result {
            Ok(sent) => {
                self.ops.add(1);
                self.bytes.add(*sent as u64);
            }
            Err(_) => {
                self.errors.add(1);
            }
        }
        result
    }
}

impl<S: crate::socket::recv::Socket> crate::socket::recv::Socket for Metered<S> {
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut core::task::Context,
        addr: &mut crate::msg::addr::Addr,
        cmsg: &mut crate::msg::cmsg::Receiver,
        buffer: &mut [io::IoSliceMut],
    ) -> core::task::Poll<io::Result<usize>> {
        let result = self.inner.poll_recv(cx, addr, cmsg, buffer);
        match &result {
            core::task::Poll::Ready(Ok(received)) => {
                self.ops.add(1);
                self.bytes.add(*received as u64);
            }
            core::task::Poll::Ready(Err(_)) => {
                self.errors.add(1);
            }
            core::task::Poll::Pending => {}
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn config_requires_bind_addrs() {
        let config = Config::new(Vec::new(), 1, 1, features::Gso::default());
        match config.create() {
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidInput),
            Ok(_) => panic!("empty bind_addrs should error"),
        }
    }

    #[test]
    fn config_requires_enough_bind_addrs() {
        let addrs = vec![SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into()];
        let config = Config::new(addrs, 2, 1, features::Gso::default());
        match config.create() {
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidInput),
            Ok(_) => panic!("insufficient bind_addrs should error"),
        }
    }

    #[test]
    fn config_requires_non_zero_socket_count() {
        let addrs = vec![SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into()];
        let config = Config::new(addrs, 0, 0, features::Gso::default());
        match config.create() {
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidInput),
            Ok(_) => panic!("zero socket counts should error"),
        }
    }

    #[test]
    fn config_binds_to_each_requested_addr() {
        let addrs = vec![
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into(),
        ];
        let (send_sockets, recv_sockets) = Config::new(addrs, 2, 1, features::Gso::default())
            .create()
            .expect("bind should work");
        assert_eq!(send_sockets.len(), 2);
        assert_eq!(recv_sockets.len(), 1);
    }

    #[test]
    fn config_shares_first_n_bind_addrs_between_send_and_recv() {
        let addrs = vec![
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).into(),
        ];
        let (send_sockets, recv_sockets) = Config::new(addrs, 2, 3, features::Gso::default())
            .create()
            .expect("bind should work");

        for idx in 0..2 {
            let send_local_addr = send_sockets[idx].0.local_addr().expect("send addr");
            let recv_local_addr = recv_sockets[idx].local_addr().expect("recv addr");
            assert_eq!(send_local_addr, recv_local_addr);
        }
    }
}
