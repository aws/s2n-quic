// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Socket trait implementations for common wrapper types

use crate::{
    msg::addr::Addr,
    socket::{fd::udp, BusyPoll, Gso},
};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::io::{self, IoSlice};

/// Trait for sockets that can send datagrams
pub trait Socket: crate::socket::LocalAddr {
    /// Send a message to the specified address using vectored I/O
    ///
    /// # Arguments
    /// * `addr` - Destination address
    /// * `payload` - Message payload as vectored buffers (IoSlices)
    /// * `segment_size` - GSO segment size (0 if no GSO)
    /// * `ecn` - Explicit Congestion Notification marking
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize>;

    /// Send a single buffer to an address with default ECN marking.
    fn send_to(&self, addr: &std::net::SocketAddr, data: &[u8]) -> io::Result<usize> {
        let addr = Addr::new((*addr).into());
        let iov = [IoSlice::new(data)];
        self.send_msg(&addr, &iov, 0, ExplicitCongestionNotification::NotEct)
    }
}

// Blanket implementations for common wrapper types

use crate::socket::LocalAddr;

impl LocalAddr for std::net::UdpSocket {
    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        (*self).local_addr()
    }
}

impl Socket for std::net::UdpSocket {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        udp::send(self, addr, ecn, payload, Some(segment_size), 0)
    }
}

impl<T: LocalAddr> LocalAddr for std::sync::Arc<T> {
    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        (**self).local_addr()
    }
}

impl<T: Socket> Socket for std::sync::Arc<T> {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        (**self).send_msg(addr, payload, segment_size, ecn)
    }
}

impl<T: LocalAddr> LocalAddr for Box<T> {
    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        (**self).local_addr()
    }
}

impl<T: Socket> Socket for Box<T> {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        (**self).send_msg(addr, payload, segment_size, ecn)
    }
}

impl<T> Socket for BusyPoll<T>
where
    T: udp::Socket,
{
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        udp::send(&self.0, addr, ecn, payload, Some(segment_size), 0)
    }
}

impl<S: LocalAddr> LocalAddr for Gso<S> {
    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.0.local_addr()
    }
}

impl<S> Socket for Gso<S>
where
    S: Socket,
{
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        self.0.send_msg(addr, payload, segment_size, ecn)
    }
}
