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
    os::fd::AsRawFd,
};
use tokio::io::unix::{AsyncFd, TryIoError};

trait UdpSocket: 'static + AsRawFd + Send + Sync {
    fn local_addr(&self) -> io::Result<SocketAddr>;
}

impl UdpSocket for std::net::UdpSocket {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (*self).local_addr()
    }
}

impl UdpSocket for tokio::net::UdpSocket {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (*self).local_addr()
    }
}

impl<T: UdpSocket> UdpSocket for std::sync::Arc<T> {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (**self).local_addr()
    }
}

impl<T: UdpSocket> UdpSocket for Box<T> {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (**self).local_addr()
    }
}

impl<T> Socket for AsyncFd<T>
where
    T: UdpSocket,
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
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
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
        cx: &mut Context,
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

        loop {
            match udp::send(self.get_ref(), addr, ecn, buffer) {
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
        cx: &mut Context,
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

        loop {
            let mut socket = ready!(self.poll_write_ready(cx))?;

            let res = socket.try_io(|fd| udp::send(fd, addr, ecn, buffer));

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
