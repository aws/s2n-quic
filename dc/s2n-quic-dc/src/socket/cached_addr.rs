// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
};

/// Caches the local address to avoid repeated syscalls.
pub struct CachedAddr<S> {
    socket: S,
    local_addr: SocketAddr,
}

impl<S> CachedAddr<S> {
    pub fn new(socket: S, local_addr: SocketAddr) -> Self {
        Self { socket, local_addr }
    }
}

impl<S> std::fmt::Debug for CachedAddr<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.local_addr.fmt(f)
    }
}

impl<S> super::LocalAddr for CachedAddr<S> {
    #[inline]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }
}

impl<S: super::send::Socket> super::send::Socket for CachedAddr<S> {
    #[inline]
    fn send_msg(
        &self,
        addr: &Addr,
        payload: &[IoSlice],
        segment_size: u16,
        ecn: ExplicitCongestionNotification,
    ) -> io::Result<usize> {
        self.socket.send_msg(addr, payload, segment_size, ecn)
    }
}

impl<S: super::recv::Socket> super::recv::Socket for CachedAddr<S> {
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        self.socket.poll_recv(cx, addr, cmsg, buffer)
    }
}
