// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg::{addr::Addr, cmsg},
    tracing::trace,
};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};

/// Trace logs every socket operation.
pub struct Tracing<S, K> {
    socket: S,
    key: K,
}

impl<S, K> Tracing<S, K> {
    pub fn new(socket: S, key: K) -> Self {
        Self { socket, key }
    }
}

impl<S: std::fmt::Debug, K> std::fmt::Debug for Tracing<S, K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.socket.fmt(f)
    }
}

impl<S: super::LocalAddr, K> super::LocalAddr for Tracing<S, K> {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

impl<S: super::send::Socket, K: std::fmt::Display> super::send::Socket for Tracing<S, K> {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        let result = self.socket.send_msg(addr, payload, segment_size, ecn);

        trace!(
            key = %self.key,
            local_addr = %self.socket.local_addr().unwrap_or_else(|_| ([0, 0, 0, 0], 0).into()),
            peer_addr = %addr.get(),
            ?ecn,
            segments = payload.len(),
            segment_size,
            total_len = payload.iter().map(|s| s.len()).sum::<usize>(),
            ?result,
            "send"
        );

        result
    }
}

impl<S: super::recv::Socket, K: std::fmt::Display + Send + 'static> super::recv::Socket
    for Tracing<S, K>
{
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        let result = self.socket.poll_recv(cx, addr, cmsg, buffer);

        if let Poll::Ready(ref res) = result {
            trace!(
                key = %self.key,
                local_addr = %self.socket.local_addr().unwrap_or_else(|_| ([0, 0, 0, 0], 0).into()),
                peer_addr = %addr.get(),
                total_len = res.as_ref().copied().unwrap_or(0),
                success = res.is_ok(),
                "recv"
            );
        }

        result
    }
}
