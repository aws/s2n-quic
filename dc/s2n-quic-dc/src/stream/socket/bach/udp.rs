// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use crate::msg::{addr::Addr, cmsg};
use bach::net::{
    socket::{RecvOptions, SendOptions},
    UdpSocket,
};
use core::task::{Context, Poll};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification, ready};
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};

impl Socket for UdpSocket {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.local_addr()
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
        let buffer = &mut [0];
        let buffer = &mut [IoSliceMut::new(buffer)];

        let mut opts = RecvOptions::default();
        opts.peek = true;

        let _res = ready!(self.poll_recv_msg(cx, buffer, opts))?;

        Ok(()).into()
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

        let mut opts = RecvOptions::default();
        opts.gro = true;

        let res = ready!(self.poll_recv_msg(cx, buffer, opts))?;

        addr.set(res.peer_addr.into());
        cmsg.set_segment_len(res.segment_len as _);
        cmsg.set_ecn(ExplicitCongestionNotification::new(res.ecn));

        Ok(res.len).into()
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

        let addr: SocketAddr = addr.get().into();
        let mut opts = SendOptions::default();
        opts.ecn = ecn as u8;

        // if we have more than 1 segment then it's GSO
        if buffer.len() > 1 {
            opts.segment_len = Some(buffer[0].len());
        }

        self.try_send_msg(addr, buffer, opts)
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

        let addr: SocketAddr = addr.get().into();
        let mut opts = SendOptions::default();
        opts.ecn = ecn as u8;

        // if we have more than 1 segment then it's GSO
        if buffer.len() > 1 {
            opts.segment_len = Some(buffer[0].len());
        }

        self.poll_send_msg(cx, addr, buffer, opts)
    }

    #[inline]
    fn send_finish(&self) -> io::Result<()> {
        // UDP sockets don't need a shut down
        Ok(())
    }
}
