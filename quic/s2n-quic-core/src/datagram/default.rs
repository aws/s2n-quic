// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

//use core::task::{Context, Poll};

use crate::datagram::{Packet, Sender};
use alloc::collections::VecDeque;
use bytes::Bytes;

#[derive(Debug, Default)]
pub struct DefaultSender {
    pub queue: VecDeque<Datagram>,
    capacity: usize,
}

#[derive(Debug)]
pub struct Datagram {
    pub data: Bytes,
}

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Cede space to stream data when datagrams are not prioritized
        if packet.has_pending_streams() && !packet.datagrams_prioritized() {
            return;
        }

        let mut has_written = false;
        while packet.remaining_capacity() > 0 {
            if let Some(datagram) = self.queue.pop_front() {
                // Ensure there is enough space in the packet to send a datagram
                if packet.remaining_capacity() >= datagram.data.len() {
                    match packet.write_datagram(&datagram.data) {
                        Ok(()) => has_written = true,
                        Err(_error) => {
                            // TODO emit datagram dropped event
                            continue;
                        }
                    }
                } else {
                    // This check keeps us from popping all the datagrams off the
                    // queue when packet space remaining is smaller than the datagram.
                    if has_written {
                        self.queue.push_front(datagram);
                        return;
                    } else {
                        // TODO emit datagram dropped event
                    }
                }
            } else {
                // If there are no datagrams on the queue we return
                return;
            }
        }
    }

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        !self.queue.is_empty()
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum SendDatagramError {
    QueueAtCapacity,
}

impl DefaultSender {
    /// Creates a builder for the default datagram sender
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Enqueues a datagram for sending it towards the peer.
    ///
    /// # Return value
    ///
    /// The function returns:
    ///
    /// - `Poll::Pending` if the datagram's send buffer capacity is currently exhausted. In this case,
    ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
    ///   [`Context`](core::task::Context) is notified.
    /// - `Poll::Ready(Ok(()))` if the datagram was enqueued for sending.
    /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
    // pub fn poll_send_datagram(
    //     &mut self,
    //     data: bytes::Bytes,
    //     cx: &mut Context,
    // ) -> Poll<Result<(), ()>> {
    //     if self.queue.len() == self.capacity {
    //         cx.waker().wake_by_ref();
    //         return Poll::Pending;
    //     }

    //     let datagram = Datagram { data };
    //     self.queue.push_back(datagram);
    //     Poll::Ready(Ok(()))
    // }

    /// Adds datagrams on the queue to be sent
    ///
    /// If the datagram queue is at capacity the oldest datagram will be popped
    /// off the queue and returned to make space for the newest datagram.
    pub fn send_datagram(&mut self, data: bytes::Bytes) -> Option<Datagram> {
        // Pop oldest datagram off the queue if it is at capacity
        if self.queue.len() == self.capacity {
            return self.queue.pop_front();
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);
        None
    }

    /// Adds datagrams on the queue to be sent
    ///
    /// If the queue is full the newest datagram is not added and an error is returned.
    pub fn send_datagram_with_error(
        &mut self,
        data: bytes::Bytes,
    ) -> Result<(), SendDatagramError> {
        if self.queue.len() == self.capacity {
            return Err(SendDatagramError::QueueAtCapacity);
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);
        Ok(())
    }

    /// Filter through the datagrams in the send queue and only keep those that
    /// match a predicate
    pub fn retain_datagrams<F>(&mut self, f: F)
    where
        F: FnMut(&Datagram) -> bool,
    {
        self.queue.retain(f);
    }
}

/// A builder for the default datagram sender
///
/// Use to configure a datagram send queue size
#[derive(Debug)]
pub struct Builder {
    queue_capacity: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            queue_capacity: 200,
        }
    }
}

impl Builder {
    /// Sets the capacity of the datagram sender queue
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }
    /// Builds the datagram sender into a provider
    pub fn build(self) -> Result<DefaultSender, core::convert::Infallible> {
        Ok(DefaultSender {
            queue: VecDeque::with_capacity(self.queue_capacity),
            capacity: self.queue_capacity,
        })
    }
}
