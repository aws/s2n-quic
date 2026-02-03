// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::super::{Protocol, Socket, TransportFeatures};
use crate::msg::{addr::Addr, cmsg};
use core::{
    future::Future,
    task::{Context, Poll},
};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr, pin::pin,
};
use turmoil::net::UdpSocket;

impl Socket for UdpSocket {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        UdpSocket::local_addr(self)
    }

    #[inline]
    fn protocol(&self) -> Protocol {
        Protocol::Udp
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn poll_peek_ready(&self, cx: &mut Context) -> Poll<io::Result<()>> {
        // Turmoil doesn't have a peek method, so we poll readable
        let readable_fut = pin!(self.readable());
        readable_fut.poll(cx)
    }

    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        _cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to recv into an empty buffer"
        );

        let mut total = 0;
        for buf in buffer {
            if buf.is_empty() {
                continue;
            }
            match poll_recv_one(self, cx, buf) {
                Poll::Ready(Ok((len, peer_addr))) => {
                    if total == 0 {
                        // Set the peer_addr on the first result. This follows the 
                        // behavior of the UDP socket with msghdr.
                        addr.set(peer_addr.into());
                    }
                    total += len;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending if total > 0 => return Poll::Ready(Ok(total)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(total))
    }

    #[inline]
    fn try_send(
        &self,
        addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        ensure!(!buffer.is_empty(), Ok(0));

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to send from an empty buffer"
        );

        debug_assert!(
            addr.get().port() != 0,
            "cannot send packet to unspecified port"
        );

        let peer_addr: SocketAddr = addr.get().into();

        let mut total = 0;
        for buf in buffer {
            if buf.is_empty() {
                continue;
            }
            match self.try_send_to(buf, peer_addr) {
                Ok(len) => total += len,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }

    #[inline]
    fn poll_send(
        &self,
        cx: &mut Context,
        addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to send from an empty buffer"
        );

        debug_assert!(
            addr.get().port() != 0,
            "cannot send packet to unspecified port"
        );

        let peer_addr: SocketAddr = addr.get().into();

        let mut total = 0;
        for buf in buffer {
            if buf.is_empty() {
                continue;
            }
            match poll_send_one(self, cx, buf, peer_addr) {
                Poll::Ready(Ok(len)) => total += len,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending if total > 0 => return Poll::Ready(Ok(total)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(total))
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}

/// Polls for a single recv operation, handling WouldBlock by re-polling readiness
fn poll_recv_one(
    socket: &UdpSocket,
    cx: &mut Context,
    buf: &mut [u8],
) -> Poll<io::Result<(usize, SocketAddr)>> {
    loop {
        // pinning readable is acceptable, since the poll does not hold the buffer or state
        // that can be dropped on cancellation.
        let Poll::Ready(result) = pin!(socket.readable()).poll(cx) else {
            return Poll::Pending;
        };
        result?;

        match socket.try_recv_from(buf) {
            Ok((len, peer_addr)) => return Poll::Ready(Ok((len, peer_addr))),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Poll::Ready(Err(e)),
        }
    }
}

/// Polls for a single send operation, handling WouldBlock by re-polling writability
fn poll_send_one(
    socket: &UdpSocket,
    cx: &mut Context,
    buf: &[u8],
    peer_addr: SocketAddr,
) -> Poll<io::Result<usize>> {
    loop {
        // pinning writable is acceptable, since the poll does not hold the buffer or state
        // that can be dropped on cancellation.
        let Poll::Ready(result) = pin!(socket.writable()).poll(cx) else {
            return Poll::Pending;
        };
        result?;

        match socket.try_send_to(buf, peer_addr) {
            Ok(len) => return Poll::Ready(Ok(len)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Poll::Ready(Err(e)),
        }
    }
}