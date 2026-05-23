// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg::{addr::Addr, cmsg},
    socket::{
        fd::{self, udp},
        BusyPoll,
    },
};
use core::task::{Context, Poll};
use s2n_quic_core::ensure;
use std::{io, io::IoSliceMut};

/// A socket that can receive packets
pub trait Socket: crate::socket::LocalAddr + Send + 'static {
    /// Polls for receiving data
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>>;
}

impl<T: Socket + Sync> Socket for std::sync::Arc<T> {
    #[inline]
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        (**self).poll_recv(cx, addr, cmsg, buffer)
    }
}

impl<T> Socket for BusyPoll<T>
where
    T: udp::Socket,
{
    #[inline]
    fn poll_recv(
        &self,
        _cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>> {
        ensure!(!buffer.is_empty(), Ok(0).into());

        debug_assert!(
            buffer.iter().any(|s| !s.is_empty()),
            "trying to recv into an empty buffer"
        );

        loop {
            let res = udp::recv(&self.0, addr, cmsg, buffer, fd::Flags::default());

            match res {
                Ok(0) => continue,
                Ok(len) => return Ok(len).into(),
                Err(ref e)
                    if [io::ErrorKind::WouldBlock, io::ErrorKind::Interrupted]
                        .contains(&e.kind()) =>
                {
                    return Poll::Pending;
                }
                Err(err) => return Err(err).into(),
            }
        }
    }
}
