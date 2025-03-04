// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    socket::recv::{descriptor, pool, router::Router},
    stream::socket::{fd::udp, Socket},
};
use std::{collections::VecDeque, io, os::fd::AsRawFd, task::Poll};

pub struct Allocator {
    queue: VecDeque<pool::Pool>,
    max_packet_size: u16,
    packet_count: usize,
}

impl Allocator {
    pub fn new(max_packet_size: u16, packet_count: usize) -> Self {
        // The Pool struct size is quite small so start off with 16 in case we need the space later
        let mut queue = VecDeque::with_capacity(16);
        queue.push_back(pool::Pool::new(max_packet_size, packet_count));
        Self {
            queue,
            max_packet_size,
            packet_count,
        }
    }

    #[inline]
    fn alloc(&mut self) -> descriptor::Unfilled {
        let mut rotate_count = 0;

        // search through the list for a pool with a free segment
        while rotate_count < self.queue.len() {
            let front = self.queue.front_mut().unwrap();
            if let Some(message) = front.alloc() {
                return message;
            }

            self.queue.rotate_left(1);
            rotate_count += 1;
        }

        // we've exhausted all of the current pools so create a new one
        let pool = pool::Pool::new(self.max_packet_size, self.packet_count);
        let desc = pool.alloc().unwrap();
        self.queue.push_front(pool);
        desc
    }
}

/// Receives packets from a blocking [`UdpSocket`] and dispatches into the provided [`Router`]
pub fn blocking<S: AsRawFd, R: Router>(socket: S, mut alloc: Allocator, mut router: R) {
    while router.is_open() {
        let mut unfilled = alloc.alloc();
        while router.is_open() {
            let res = unfilled.recv_with(|addr, cmsg, buffer| {
                udp::recv(&socket, addr, cmsg, &mut [buffer], Default::default())
            });

            match res {
                Ok(segments) => {
                    for segment in segments {
                        router.on_segment(segment);
                    }
                    break;
                }
                Err((desc, err)) => {
                    tracing::error!("socket recv error: {err}");
                    unfilled = desc;
                    continue;
                }
            }
        }
    }
}

/// Receives packets from a blocking [`UdpSocket`] and dispatches into the provided [`Router`]
pub async fn non_blocking<S: Socket, R: Router>(socket: S, mut alloc: Allocator, mut router: R) {
    let mut pending = None;
    core::future::poll_fn(move |cx| {
        while router.is_open() {
            let unfilled = pending.take().unwrap_or_else(|| alloc.alloc());

            let res = unfilled.recv_with(|addr, cmsg, buffer| {
                match socket.poll_recv(cx, addr, cmsg, &mut [buffer]) {
                    Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
                    Poll::Ready(Ok(len)) => Ok(len),
                    Poll::Ready(Err(err)) => Err(err),
                }
            });

            match res {
                Ok(segments) => {
                    for segment in segments {
                        router.on_segment(segment);
                    }

                    // poll the socket again
                    continue;
                }
                Err((desc, err)) => {
                    // put the unfilled segment back in the pool
                    pending = Some(desc);

                    // if we got blocked then yield the future
                    if err.kind() == io::ErrorKind::WouldBlock {
                        return Poll::Pending;
                    }

                    tracing::error!("socket recv error: {err}");
                }
            }
        }

        Poll::Ready(())
    })
    .await;
}
