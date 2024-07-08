// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};
use tracing::trace;

pub struct Tracing<S: Socket>(pub S);

impl<S: Socket> Socket for Tracing<S> {
    #[inline(always)]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    #[inline]
    fn protocol(&self) -> Protocol {
        self.0.protocol()
    }

    #[inline(always)]
    fn features(&self) -> TransportFeatures {
        self.0.features()
    }

    #[inline(always)]
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
        let result = self.0.poll_peek_len(cx);

        trace!(
            operation = %"poll_peek_len",
            protocol = ?self.protocol(),
            local_addr = ?self.local_addr(),
            result = ?result,
        );

        result
    }

    #[inline(always)]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        let result = self.0.poll_recv(cx, addr, cmsg, buffer);

        match &result {
            Poll::Ready(Ok(_)) => trace!(
                operation = %"poll_recv",
                protocol = ?self.protocol(),
                local_addr = ?self.local_addr(),
                remote_addr = ?addr,
                ecn = ?cmsg.ecn(),
                segments = buffer.len(),
                segment_len = cmsg.segment_len(),
                buffer_len = {
                    let v: usize = buffer.iter().map(|s| s.len()).sum();
                    v
                },
                result = ?result,
            ),
            _ => trace!(
                operation = %"poll_recv",
                protocol = ?self.protocol(),
                local_addr = ?self.local_addr(),
                segments = buffer.len(),
                buffer_len = {
                    let v: usize = buffer.iter().map(|s| s.len()).sum();
                    v
                },
                result = ?result,
            ),
        }

        result
    }

    #[inline(always)]
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        let result = self.0.try_send(addr, ecn, buffer);

        trace!(
            operation = %"try_send",
            protocol = ?self.protocol(),
            local_addr = ?self.local_addr(),
            remote_addr = ?addr,
            ?ecn,
            segments = buffer.len(),
            segment_len = buffer.first().map_or(0, |s| s.len()),
            buffer_len = {
                let v: usize = buffer.iter().map(|s| s.len()).sum();
                v
            },
            result = ?result,
        );

        result
    }

    #[inline(always)]
    fn poll_send(
        &self,
        cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        let result = self.0.poll_send(cx, addr, ecn, buffer);

        trace!(
            operation = %"poll_send",
            protocol = ?self.protocol(),
            local_addr = ?self.local_addr(),
            remote_addr = ?addr,
            ?ecn,
            segments = buffer.len(),
            segment_len = buffer.first().map_or(0, |s| s.len()),
            buffer_len = {
                let v: usize = buffer.iter().map(|s| s.len()).sum();
                v
            },
            result = ?result,
        );

        result
    }

    #[inline(always)]
    fn send_finish(&self) -> io::Result<()> {
        let result = self.0.send_finish();

        trace!(
            operation = %"send_finish",
            protocol = ?self.protocol(),
            local_addr = ?self.local_addr(),
            result = ?result,
        );

        result
    }
}
