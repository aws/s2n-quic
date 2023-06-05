// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::Gso,
    socket::{
        ring,
        task::{rx, tx},
    },
    syscall::{SocketType, UnixMessage},
};
use core::task::{Context, Poll};
use std::{io, os::unix::io::AsRawFd};
use tokio::io::unix::AsyncFd;

pub async fn rx<S: Into<std::net::UdpSocket>, M: UnixMessage + Unpin>(
    socket: S,
    producer: ring::Producer<M>,
) -> io::Result<()> {
    let socket = socket.into();
    socket.set_nonblocking(true).unwrap();

    let socket = AsyncFd::new(socket).unwrap();
    let result = rx::Receiver::new(producer, socket).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

pub async fn tx<S: Into<std::net::UdpSocket>, M: UnixMessage + Unpin>(
    socket: S,
    consumer: ring::Consumer<M>,
    gso: Gso,
) -> io::Result<()> {
    let socket = socket.into();
    socket.set_nonblocking(true).unwrap();

    let socket = AsyncFd::new(socket).unwrap();
    let result = tx::Sender::new(consumer, socket, gso).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

impl<S: AsRawFd, M: UnixMessage> tx::Socket<M> for AsyncFd<S> {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        cx: &mut Context,
        entries: &mut [M],
        events: &mut tx::Events,
    ) -> io::Result<()> {
        M::send(self.get_ref().as_raw_fd(), entries, events);

        if !events.is_blocked() {
            return Ok(());
        }

        for i in 0..2 {
            match self.poll_write_ready(cx) {
                Poll::Ready(guard) => {
                    let mut guard = guard?;
                    if i == 0 {
                        guard.clear_ready();
                    } else {
                        events.take_blocked();
                    }
                }
                Poll::Pending => {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}

impl<S: AsRawFd, M: UnixMessage> rx::Socket<M> for AsyncFd<S> {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [M],
        events: &mut rx::Events,
    ) -> io::Result<()> {
        M::recv(
            self.get_ref().as_raw_fd(),
            SocketType::NonBlocking,
            entries,
            events,
        );

        if !events.is_blocked() {
            return Ok(());
        }

        for i in 0..2 {
            match self.poll_read_ready(cx) {
                Poll::Ready(guard) => {
                    let mut guard = guard?;
                    if i == 0 {
                        guard.clear_ready();
                    } else {
                        events.take_blocked();
                    }
                }
                Poll::Pending => {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}
