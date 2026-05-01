// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    fd::{udp, Flags},
    Protocol, Socket, TransportFeatures,
};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
    os::fd::{AsRawFd, RawFd},
};

pub struct BusyPoll<T>(pub T);

impl<T> udp::Socket for BusyPoll<T>
where
    T: udp::Socket,
{
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl<T> AsRawFd for BusyPoll<T>
where
    T: udp::Socket,
{
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl<T> Socket for BusyPoll<T>
where
    T: udp::Socket,
{
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
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
    fn poll_peek_len(&self, _cx: &mut Context) -> Poll<io::Result<usize>> {
        let res = udp::peek(&self.0);

        match res {
            Ok(len) => return Ok(len).into(),
            Err(ref e)
                if [io::ErrorKind::WouldBlock, io::ErrorKind::Interrupted].contains(&e.kind()) =>
            {
                return Poll::Pending;
            }
            Err(err) => return Err(err).into(),
        }
    }

    #[inline]
    fn poll_recv(
        &self,
        _cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        // no point in receiving empty packets
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to recv into an empty buffer"
        );

        loop {
            let flags = Flags::default();

            let res = udp::recv(&self.0, addr, cmsg, buffer, flags);

            match res {
                Ok(0) => {
                    // no point in processing empty UDP packets
                    continue;
                }
                Ok(len) => return Ok(len).into(),
                Err(ref e)
                    if [io::ErrorKind::WouldBlock, io::ErrorKind::Interrupted]
                        .contains(&e.kind()) =>
                {
                    return Poll::Pending;
                }
                Err(err) => return Err(err).into(),
            }
        }
    }

    #[inline]
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        // no point in sending empty packets
        ensure!(!buffer.is_empty(), Ok(0));

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to send from an empty buffer"
        );

        debug_assert!(
            addr.get().port() != 0,
            "cannot send packet to unspecified port"
        );

        let res = udp::send(&self.0, addr, ecn, buffer, None, Default::default());

        if let Err(error) = &res {
            tracing::warn!(%error, "socket try_send error");
        }

        res
    }

    #[inline]
    fn poll_send(
        &self,
        _cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        // no point in sending empty packets
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to send from an empty buffer"
        );

        debug_assert!(
            addr.get().port() != 0,
            "cannot send packet to unspecified port"
        );

        let res = udp::send(&self.0, addr, ecn, buffer, None, Default::default());

        match res {
            Ok(len) => return Ok(len).into(),
            Err(ref e)
                if [io::ErrorKind::WouldBlock, io::ErrorKind::Interrupted].contains(&e.kind()) =>
            {
                return Poll::Pending;
            }
            Err(err) => return Err(err).into(),
        }
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}
