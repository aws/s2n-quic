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
    net::SocketAddr,
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
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
        // Turmoil doesn't have a peek method, so we poll readable and return a placeholder
        let fut = self.readable();
        tokio::pin!(fut);
        match fut.poll(cx) {
            Poll::Ready(Ok(())) => {
                // Socket is readable, but we can't peek - return a reasonable buffer size
                Poll::Ready(Ok(65535))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
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

        // First check if socket is readable
        let readable_fut = self.readable();
        tokio::pin!(readable_fut);
        match readable_fut.poll(cx) {
            Poll::Ready(Ok(())) => {
                // Socket is readable, try to receive
                // Turmoil doesn't support vectored I/O, so we receive into the first buffer
                match self.try_recv_from(&mut buffer[0]) {
                    Ok((len, peer_addr)) => {
                        addr.set(peer_addr.into());
                        Poll::Ready(Ok(len))
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
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

        // Turmoil doesn't support vectored I/O, so we send the first buffer
        self.try_send_to(&buffer[0], peer_addr)
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

        // First check if socket is writable
        let writable_fut = self.writable();
        tokio::pin!(writable_fut);
        match writable_fut.poll(cx) {
            Poll::Ready(Ok(())) => {
                // Socket is writable, try to send
                // Turmoil doesn't support vectored I/O, so we send the first buffer
                match self.try_send_to(&buffer[0], peer_addr) {
                    Ok(len) => Poll::Ready(Ok(len)),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}
