// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::datagram::{Packet, Sender};
use alloc::collections::VecDeque;
use bytes::Bytes;

#[derive(Debug, Default)]
pub struct DefaultSender {
    queue: VecDeque<Datagram>,
}

#[derive(Debug)]
pub struct Datagram {
    data: Bytes,
}

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Cede space to stream data when datagrams are not prioritized
        if packet.has_pending_streams() && !packet.datagrams_prioritized() {
            return;
        }

        while packet.remaining_capacity() > 0 {
            if let Some(datagram) = self.queue.pop_front() {
                // Ensure there is enough space in the packet to send a datagram
                if packet.remaining_capacity() >= datagram.data.len() {
                    match packet.write_datagram(&datagram.data) {
                        Ok(()) => continue,
                        Err(_error) => {
                            // TODO emit datagram dropped event
                            continue;
                        }
                    }
                } else {
                    // TODO emit datagram dropped event
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
pub enum SendDatagramError {}

impl DefaultSender {
    /// Creates a builder for the default datagram sender
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Adds datagrams on the queue to be sent
    pub fn send_datagram(&mut self, data: bytes::Bytes) -> Result<(), SendDatagramError> {
        // Pop oldest datagram off the queue if it is at capacity
        if self.queue.capacity() == 0 {
            self.queue.pop_front();
            // TODO emit datagram dropped event
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
        })
    }
}
