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
pub trait Socket {
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

    /// Get the local address of this socket
    fn local_addr(&self) -> io::Result<std::net::SocketAddr>;

    /// Send a single buffer to an address with default ECN marking.
    fn send_to(&self, addr: &std::net::SocketAddr, data: &[u8]) -> io::Result<usize> {
        let addr = Addr::new((*addr).into());
        let iov = [IoSlice::new(data)];
        self.send_msg(&addr, &iov, 0, ExplicitCongestionNotification::NotEct)
    }
}

// Blanket implementations for common wrapper types

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

    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        (*self).local_addr()
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

    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        (**self).local_addr()
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
        // TODO: Add GSO error handling and fallback
        // When GSO fails with EMSGSIZE or other GSO-related errors, we should:
        // 1. Disable GSO (self.1.disable())
        // 2. Retry by sending each segment individually
        // This requires refactoring handle_send_result to work without stream::Socket trait
        self.0.send_msg(addr, payload, segment_size, ecn)
    }

    #[inline]
    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.0.local_addr()
    }
}
