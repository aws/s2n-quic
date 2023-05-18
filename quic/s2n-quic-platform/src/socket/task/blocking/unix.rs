// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    socket::task::{rx, tx},
    syscall::{SocketType, UnixMessage},
};
use core::task::Context;
use std::{io, net::UdpSocket, os::unix::io::AsRawFd};

impl<M: UnixMessage> tx::Socket<M> for UdpSocket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        _cx: &mut Context,
        entries: &mut [M],
        events: &mut tx::Events,
    ) -> io::Result<()> {
        M::send(self.as_raw_fd(), entries, events);
        Ok(())
    }
}

impl<M: UnixMessage> rx::Socket<M> for UdpSocket {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        _cx: &mut Context,
        entries: &mut [M],
        events: &mut rx::Events,
    ) -> io::Result<()> {
        M::recv(self.as_raw_fd(), SocketType::Blocking, entries, events);
        Ok(())
    }
}
