// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-quic's default implementation of the datagram component

use crate::{
    datagram::datagram::{ConnectionInfo, Endpoint, Packet, Receiver, Sender},
    time::Timestamp,
};
use bytes::Bytes;
use core::time::Duration;
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct Disabled;

// Default capacity is zero right now since the feature is currently disabled.
const DEFAULT_CAPACITY: usize = 0;

impl Endpoint for Disabled {
    type Sender = DefaultSender;
    type Receiver = DisabledReceiver;

    fn create_connection(&mut self, _info: &ConnectionInfo) -> (Self::Sender, Self::Receiver) {
        let queue = VecDeque::with_capacity(DEFAULT_CAPACITY);
        (
            DefaultSender {
                queue,
                prioritize_datagrams: true,
            },
            DisabledReceiver,
        )
    }
}

pub struct DefaultSender {
    queue: VecDeque<Datagram>,
    prioritize_datagrams: bool,
}

struct Datagram {
    data: Bytes,
    creation_time: Timestamp,
}

// Expiration in ms of datagrams on the queue. If datagrams are older than 3 ms,
// consider them expired and don't send them.
const DATAGRAM_EXPIRATION: u64 = 3;

impl Sender for DefaultSender {
    fn on_transmit<P: Packet>(&mut self, packet: &mut P) {
        // Alternate between sending datagrams and ceding that space to pending
        // stream data
        let cede_space = packet.has_pending_streams() && !self.prioritize_datagrams;
        self.prioritize_datagrams = !self.prioritize_datagrams;
        if cede_space {
            return;
        }

        while packet.remaining_capacity() > 0 {
            if let Some(datagram) = self.queue.pop_front() {
                let current_time = packet.current_time();
                let elapsed_time = current_time.saturating_duration_since(datagram.creation_time);
                // Ensure there is enough space in the packet to send a datagram and the datagram is not too old
                if packet.remaining_capacity() >= datagram.data.len()
                    && elapsed_time < Duration::from_millis(DATAGRAM_EXPIRATION)
                {
                    match packet.write_datagram(&datagram.data) {
                        Ok(()) => continue,
                        Err(_error) => {
                            //TODO emit datagram dropped event
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

pub struct DisabledReceiver;

impl Receiver for DisabledReceiver {
    fn on_datagram(&self, _datagram: &[u8]) {}
}
