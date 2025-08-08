// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    fd::{udp, Flags},
    Protocol, Socket, TransportFeatures,
};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification, ready};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};
use tokio::io::unix::{AsyncFd, TryIoError};

impl<T> Socket for AsyncFd<T>
where
    T: udp::Socket,
{
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.get_ref().local_addr()
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
    fn poll_peek_len(&self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        loop {
            let mut socket = ready!(self.poll_read_ready(cx))?;

            let res = socket.try_io(udp::peek);

            match res {
                Ok(Ok(len)) => return Ok(len).into(),
                Ok(Err(ref e)) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Ok(Err(err)) => return Err(err).into(),
                Err(err) => {
                    // we got a WouldBlock so loop back around to register the waker
                    let _: TryIoError = err;
                    continue;
                }
            }
        }
    }

    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        // no point in receiving empty packets
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to recv into an empty buffer"
        );

        loop {
            let mut socket = ready!(self.poll_read_ready(cx))?;
            let flags = Flags::default();

            let res = socket.try_io(|fd| udp::recv(fd, addr, cmsg, buffer, flags));

            match res {
                Ok(Ok(0)) => {
                    // no point in processing empty UDP packets
                    continue;
                }
                Ok(Ok(len)) => return Ok(len).into(),
                Ok(Err(ref e)) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Ok(Err(err)) => return Err(err).into(),
                Err(err) => {
                    // we got a WouldBlock so loop back around to register the waker
                    let _: TryIoError = err;
                    continue;
                }
            }
        }
    }

    #[inline]
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice<'_>],
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

        loop {
            match udp::send(self.get_ref(), addr, ecn, buffer, Default::default()) {
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                res => return res,
            }
        }
    }

    #[inline]
    fn poll_send(
        &self,
        cx: &mut Context<'_>,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice<'_>],
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

        loop {
            let mut socket = ready!(self.poll_write_ready(cx))?;

            let res = socket.try_io(|fd| udp::send(fd, addr, ecn, buffer, Default::default()));

            match res {
                Ok(Ok(len)) => return Ok(len).into(),
                Ok(Err(ref e)) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Ok(Err(err)) => return Err(err).into(),
                Err(err) => {
                    // we got a WouldBlock so loop back around to register the waker
                    let _: TryIoError = err;
                    continue;
                }
            }
        }
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}
