// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, Socket, TransportFeatures};
use crate::msg::{addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use s2n_quic_platform::features;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
    ops::Deref,
};

#[derive(Clone)]
pub struct Gso<S>(pub S, pub features::Gso);

impl<S: Socket> Gso<S> {
    #[inline(always)]
    pub(crate) fn handle_send_result(
        &self,
        result: io::Result<usize>,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        let err = match result {
            Ok(result) => return Ok(result),
            Err(err) => err,
        };

        let mut did_disable = false;

        if err.raw_os_error() == Some(libc::EMSGSIZE) {
            self.1.disable();
            did_disable = true;
        }

        if !did_disable && self.1.handle_socket_error(&err).is_some() {
            did_disable = true;
        }

        if did_disable {
            let mut len = 0;
            for buffer in buffer {
                len += buffer.len();
                let _ = self.0.try_send(addr, ecn, &[*buffer]);
            }
            return Ok(len);
        }

        Err(err)
    }
}

impl<S: Socket> Deref for Gso<S> {
    type Target = S;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Socket> Socket for Gso<S> {
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
        self.0.poll_peek_len(cx)
    }

    #[inline(always)]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        self.0.poll_recv(cx, addr, cmsg, buffer)
    }

    #[inline(always)]
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize> {
        let result = self.0.try_send(addr, ecn, buffer);
        self.handle_send_result(result, addr, ecn, buffer)
    }

    #[inline(always)]
    fn poll_send(
        &self,
        cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>> {
        self.0
            .poll_send(cx, addr, ecn, buffer)
            .map(|result| self.handle_send_result(result, addr, ecn, buffer))
    }

    #[inline(always)]
    fn send_finish(&self) -> io::Result<()> {
        self.0.send_finish()
    }
}
