// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::{self, EndpointPublisher};
use core::task::{Context, Poll};
use s2n_quic_core::inet::SocketAddress;
use std::{collections::VecDeque, io};

/// Converts the kernel's TCP FIFO accept queue to LIFO
///
/// This should produce overall better latencies in the case of overloaded queues.
pub struct Queue<Stream> {
    queue: VecDeque<(Stream, SocketAddress)>,
}

impl<Stream> Queue<Stream> {
    #[inline]
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn fill<L, Pub>(&mut self, cx: &mut Context, listener: &mut L, publisher: &Pub)
    where
        L: Listener<Stream = Stream>,
        Pub: EndpointPublisher,
    {
        // Allow draining the queue twice the capacity
        //
        // The idea here is to try and reduce the number of connections in the kernel's queue while
        // bounding the amount of work we do in userspace.
        //
        // TODO: investigate getting the current length and dropping the front of the queue rather
        // than pop/push with the userspace queue
        let mut remaining = self.queue.capacity() * 2;

        let mut enqueued = 0;
        let mut dropped = 0;
        let mut errored = 0;

        while let Poll::Ready(res) = listener.poll_accept(cx) {
            match res {
                Ok((socket, remote_address)) => {
                    if self.queue.len() == self.queue.capacity() {
                        if let Some(remote_address) = self
                            .queue
                            .pop_back()
                            .map(|(_socket, remote_address)| remote_address)
                        {
                            publisher.on_acceptor_tcp_stream_dropped(
                                event::builder::AcceptorTcpStreamDropped { remote_address: &remote_address, reason: event::builder::AcceptorTcpStreamDropReason::FreshQueueAtCapacity },
                            );
                            dropped += 1;
                        }
                    }

                    publisher.on_acceptor_tcp_fresh_enqueued(
                        event::builder::AcceptorTcpFreshEnqueued {
                            remote_address: &remote_address,
                        },
                    );
                    enqueued += 1;

                    // most recent streams go to the front of the line, since they're the most
                    // likely to be successfully processed
                    self.queue.push_front((socket, remote_address));
                }
                Err(error) => {
                    // TODO submit to a separate error channel that the application can subscribe
                    // to
                    publisher.on_acceptor_tcp_io_error(event::builder::AcceptorTcpIoError {
                        error: &error,
                    });
                    errored += 1;
                }
            }

            remaining -= 1;

            if remaining == 0 {
                // if we're yielding then we need to wake ourselves up again
                cx.waker().wake_by_ref();
                break;
            }
        }

        publisher.on_acceptor_tcp_fresh_batch_completed(
            event::builder::AcceptorTcpFreshBatchCompleted {
                enqueued,
                dropped,
                errored,
            },
        )
    }

    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = (Stream, SocketAddress)> + '_ {
        self.queue.drain(..)
    }
}

pub trait Listener {
    type Stream;

    fn poll_accept(&mut self, cx: &mut Context) -> Poll<io::Result<(Self::Stream, SocketAddress)>>;
}

impl Listener for tokio::net::TcpListener {
    type Stream = tokio::net::TcpStream;

    #[inline]
    fn poll_accept(&mut self, cx: &mut Context) -> Poll<io::Result<(Self::Stream, SocketAddress)>> {
        (*self)
            .poll_accept(cx)
            .map_ok(|(socket, remote_address)| (socket, remote_address.into()))
    }
}
