// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    fd::{tcp, Flags},
    Protocol, Socket, TransportFeatures,
};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::{inet::ExplicitCongestionNotification, ready};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};
use tokio::{io::Interest, net::TcpStream};

impl Socket for TcpStream {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        (*self).local_addr()
    }

    #[inline]
    fn protocol(&self) -> Protocol {
        Protocol::Udp
    }

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::TCP
    }

    #[inline]
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.poll_read_ready(cx))?;

            let res = self.try_io(Interest::READABLE, || tcp::peek(self));

            match res {
                Ok(len) => return Ok(len).into(),
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // register the waker
                    continue;
                }
                Err(err) => return Err(err).into(),
            }
        }
    }

    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        _addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.poll_read_ready(cx))?;

            let flags = Flags::default();
            let res = self.try_io(Interest::READABLE, || tcp::recv(self, buffer, flags));

            match res {
                Ok(len) => {
                    // we don't need ECN markings from TCP since it handles that logic for us
                    cmsg.set_ecn(ExplicitCongestionNotification::NotEct);

                    // TCP doesn't have segments so just set it to 0 (which will indicate a single
                    // stream of bytes)
                    cmsg.set_segment_len(0);

                    return Ok(len).into();
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // register the waker
                    continue;
                }
                Err(err) => return Err(err).into(),
            }
        }
    }

    #[inline]
    fn try_send(
        &self,
        _addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        loop {
            match tcp::send(self, buffer) {
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
        _addr: &Addr,
        _ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.poll_write_ready(cx))?;

            let res = self.try_io(Interest::WRITABLE, || tcp::send(self, buffer));

            match res {
                Ok(len) => return Ok(len).into(),
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // register the waker
                    continue;
                }
                Err(err) => return Err(err).into(),
            }
        }
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // AsyncWrite::poll_shutdown requires a `&mut self` so we just use libc directly
        tcp::shutdown(self)
    }
}
