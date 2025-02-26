// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    socket::recv::{descriptor, pool, router::Router},
    stream::socket::fd::udp,
};
use std::{collections::VecDeque, net::UdpSocket};

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
pub fn blocking<R: Router>(socket: UdpSocket, mut alloc: Allocator, mut router: R) {
    loop {
        let mut unfilled = alloc.alloc();
        loop {
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
