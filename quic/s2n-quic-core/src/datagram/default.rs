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
    prioritize_datagrams: bool,
}

struct Datagram {
    data: Bytes,
    creation_time: Timestamp,
}

// Expiration in ms of datagrams on the queue. If datagrams are older than 3 ms,
// consider them expired and don't send them.
const DATAGRAM_EXPIRATION: Duration = Duration::from_millis(3);

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
                    && elapsed_time < DATAGRAM_EXPIRATION
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
