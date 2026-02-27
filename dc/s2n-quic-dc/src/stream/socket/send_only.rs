// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{fd::udp, Protocol, Socket, TransportFeatures};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};

#[derive(Clone, Debug)]
pub struct SendOnly<T: udp::Socket>(pub T);

impl<T> Socket for SendOnly<T>
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
    fn poll_peek_ready(&self, _cx: &mut Context) -> Poll<io::Result<()>> {
        unimplemented!()
    }

    #[inline]
    fn poll_recv(
        &self,
        _cx: &mut Context,
        _addr: &mut Addr,
        _cmsg: &mut cmsg::Receiver,
        _buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        unimplemented!()
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
            match udp::send(&self.0, addr, ecn, buffer, libc::MSG_DONTWAIT) {
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // try the operation again if we were interrupted
                    continue;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // we got a WouldBlock so pretend we sent it - we have no way of registering interest
                    return Ok(buffer.iter().map(|s| s.len()).sum());
                }
                res => return res,
            }
        }
    }

    #[inline]
    fn poll_send(
        &self,
        _cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        self.try_send(addr, ecn, buffer).into()
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}
