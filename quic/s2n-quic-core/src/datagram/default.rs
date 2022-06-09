// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::datagram::{Packet, Sender};
use alloc::collections::VecDeque;
use bytes::Bytes;

#[derive(Debug)]
pub struct DefaultSender {
    pub queue: VecDeque<Datagram>,
    capacity: usize,
    min_packet_space: usize,
    max_packet_space: usize,
    smoothed_packet_size: f64,
}

#[derive(Debug, PartialEq)]
pub struct Datagram {
    pub data: Bytes,
}

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Cede space to stream data when datagrams are not prioritized
        if packet.has_pending_streams() && !packet.datagrams_prioritized() {
            return;
        }
        DefaultSender::record_capacity_stats(self, packet.remaining_capacity());
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
#[derive(Debug, PartialEq)]
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
    // pub fn poll_send_datagram(
    //     &mut self,
    //     data: bytes::Bytes,
    //     cx: &mut Context,
    // ) -> Poll<Result<(), ()>> {
    //     if self.queue.len() == self.capacity {
    //         self.waker = Some(cx.waker());
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
        let mut oldest = None;
        if self.queue.len() == self.capacity {
            oldest = self.queue.pop_front();
        }

        let datagram = Datagram { data };
        self.queue.push_back(datagram);
        oldest
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

    fn record_capacity_stats(&mut self, capacity: usize) {
        if capacity < self.min_packet_space || self.min_packet_space == 0 {
            self.min_packet_space = capacity;
        }
        if capacity > self.max_packet_space {
            self.max_packet_space = capacity;
        }

        // https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        self.smoothed_packet_size =
            7.0 / 8.0 * self.smoothed_packet_size + 1.0 / 8.0 * capacity as f64;
    }

    /// Returns the largest packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn max_packet_space(&self) -> usize {
        self.max_packet_space
    }

    /// Returns the smallest packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn min_packet_space(&self) -> usize {
        self.min_packet_space
    }

    /// Returns a smoothed calculation of the size of packet space for datagrams seen during this connection.
    ///
    /// Should be used to determine an appropriate datagram size that can be sent in
    /// this connection.
    pub fn smoothed_packet_space(&self) -> f64 {
        self.smoothed_packet_size
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
            max_packet_space: 0,
            min_packet_space: 0,
            smoothed_packet_size: 0.0,
        })
    }
}

#[test]
fn send_datagram() {
    // Create a default sender queue that only holds two elements
    let mut default_sender = DefaultSender::builder().with_capacity(2).build().unwrap();
    let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
    let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
    let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
    default_sender.send_datagram(datagram_0);
    default_sender.send_datagram(datagram_1);
    default_sender.send_datagram(datagram_2);

    // Oldest datagram has been bumped off the queue and the newest two datagrams
    // are there
    let second = default_sender.queue.pop_front().unwrap();
    assert_eq!(second.data[..], [4, 5, 6]);
    let third = default_sender.queue.pop_front().unwrap();
    assert_eq!(third.data[..], [7, 8, 9]);
    assert!(default_sender.queue.is_empty());
}

#[test]
fn send_datagram_with_error() {
    // Create a default sender queue that only holds two elements
    let mut default_sender = DefaultSender::builder().with_capacity(2).build().unwrap();
    let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
    let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
    let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
    default_sender.send_datagram(datagram_0);
    default_sender.send_datagram(datagram_1);
    // Attempting to send a third datagram will result in an error, since the queue
    // is at capacity
    assert_eq!(
        default_sender.send_datagram_with_error(datagram_2),
        Err(SendDatagramError::QueueAtCapacity)
    );

    // Check that the first two datagrams are still there
    let first = default_sender.queue.pop_front().unwrap();
    assert_eq!(first.data[..], [1, 2, 3]);
    let second = default_sender.queue.pop_front().unwrap();
    assert_eq!(second.data[..], [4, 5, 6]);
    assert!(default_sender.queue.is_empty());
}

#[test]
fn retain_datagrams() {
    // Create a default sender queue
    let mut default_sender = DefaultSender::builder().with_capacity(2).build().unwrap();
    let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
    let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
    let datagram_2 = bytes::Bytes::from_static(&[7, 8, 9]);
    default_sender.send_datagram(datagram_0);
    default_sender.send_datagram(datagram_1);
    default_sender.send_datagram(datagram_2);

    // Keep only the third datagram
    default_sender.retain_datagrams(|datagram| datagram.data[..] == [7, 8, 9]);
    let first = default_sender.queue.pop_front().unwrap();
    assert_eq!(first.data[..], [7, 8, 9]);
    assert!(default_sender.queue.is_empty());
}

#[test]
fn record_capacity_stats() {
    // Create a default sender queue
    let mut default_sender = DefaultSender::builder().build().unwrap();
    default_sender.record_capacity_stats(100);
    default_sender.record_capacity_stats(100);
    default_sender.record_capacity_stats(200);
    default_sender.record_capacity_stats(500);

    assert_eq!(default_sender.max_packet_space(), 500);
    assert_eq!(default_sender.min_packet_space(), 100);

    // There's no test for a correct calculation of a weighted average, but we
    // can at least check our output isn't zero
    assert!(default_sender.smoothed_packet_space() > 0.0);
}
