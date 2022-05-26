// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::{
    datagram::{ConnectionInfo, Endpoint, Packet, Receiver, Sender},
    event::Timestamp,
};
use alloc::collections::VecDeque;
use bytes::Bytes;
use core::time::Duration;

#[derive(Debug, Default)]
pub struct Disabled;

impl Endpoint for Disabled {
    type Sender = DisabledSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        (DisabledSender, DisabledReceiver)
    }
}

pub struct DisabledSender;
pub struct DisabledReceiver;

impl Sender for DisabledSender {
    fn on_transmit<P: Packet>(&mut self, _packet: &mut P) {}

    #[inline]
    fn has_transmission_interest(&self) -> bool {
        false
    }
}

impl Receiver for DisabledReceiver {
    fn on_datagram(&self, _datagram: &[u8]) {}
}

pub struct DefaultSender {
    queue: VecDeque<Datagram>,
    datagram_expiration: Duration,
}

struct Datagram {
    data: Bytes,
    creation_time: Option<Timestamp>,
}

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Cede space to stream data when datagrams are not prioritized
        if packet.has_pending_streams() && !packet.datagrams_prioritized() {
            return;
        }

        while packet.remaining_capacity() > 0 {
            if let Some(datagram) = self.queue.pop_front() {
                // Ensure datagram is not too old
                if let Some(creation_time) = datagram.creation_time {
                    let current_time = packet.current_time();
                    let elapsed_time = current_time.saturating_duration_since(creation_time);
                    if elapsed_time > self.datagram_expiration {
                        // TODO emit datagram dropped event
                        continue;
                    }
                }

                // Ensure there is enough space in the packet to send a datagram
                if packet.remaining_capacity() >= datagram.data.len() {
                    match packet.write_datagram(&datagram.data) {
                        Ok(()) => continue,
                        Err(_error) => {
                            // TODO emit datagram dropped event
                            continue;
                        }
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

impl DefaultSender {
    /// Creates a builder for the default datagram sender
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// A builder for the default datagram sender
///
/// Use to configure a datagram expiration time and send queue size
#[derive(Debug)]
pub struct Builder {
    datagram_expiration: Duration,
    queue_capacity: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            // Todo!
            datagram_expiration: Duration::from_millis(0),
            queue_capacity: 0,
        }
    }
}

impl Builder {
    /// Sets the capacity of the datagram sender queue
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity;
        self
    }

    /// Sets the expiration time of a datagram
    pub fn with_lifetime(mut self, expiration: Duration) -> Self {
        self.datagram_expiration = expiration;
        self
    }

    /// Builds the datagram sender into a provider
    pub fn build(self) -> Result<DefaultSender, core::convert::Infallible> {
        Ok(DefaultSender {
            queue: VecDeque::with_capacity(self.queue_capacity),
            datagram_expiration: self.datagram_expiration,
        })
    }
}
