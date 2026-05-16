// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::msg::{addr::Addr, cmsg};
use bach::net::{socket::RecvOptions, UdpSocket};
use core::task::{Context, Poll};
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification, ready};
use std::{
    io::{self, IoSliceMut},
    net::SocketAddr,
};

impl super::Socket for UdpSocket {
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
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
    fn local_addr(&self) -> io::Result<SocketAddr> {
        UdpSocket::local_addr(self)
    }
}
