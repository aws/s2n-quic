// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use s2n_quic_core::{
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet,
    path::MINIMUM_MTU,
    random, stateless_reset, time,
};

#[derive(Debug)]
pub struct Dispatch {
    transmissions: VecDeque<Transmission>,
}

impl Default for Dispatch {
    fn default() -> Self {
        Self::new(endpoint::DEFAULT_MAX_PEERS)
    }
}

impl Dispatch {
    pub fn new(max_peers: usize) -> Self {
        Self {
            transmissions: VecDeque::with_capacity(max_peers),
        }
    }

    pub fn queue<R: random::Generator>(
        &mut self,
        token: stateless_reset::Token,
        max_tag_len: usize,
        triggering_packet_len: usize,
        random_generator: &mut R,
        datagram: &DatagramInfo,
    ) {
        if let Some(transmission) = Transmission::new(
            datagram.remote_address,
            token,
            max_tag_len,
            triggering_packet_len,
            random_generator,
        ) {
            self.transmissions.push_back(transmission);
        }
    }

    pub fn on_transmit<Tx: tx::Queue>(&mut self, queue: &mut Tx) {
        while let Some(transmission) = self.transmissions.pop_front() {
            if queue.push(&transmission).is_err() {
                self.transmissions.push_front(transmission);
                return;
            }
        }
    }
}

pub struct Transmission {
    remote_address: SocketAddress,
    packet: [u8; MINIMUM_MTU as usize],
    packet_len: usize,
}

impl core::fmt::Debug for Transmission {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.remote_address)
            .field("packet_len", &self.packet_len)
            .field("packet", &&self.packet[0..self.packet_len])
            .finish()
    }
}

impl Transmission {
    pub fn new<R: random::Generator>(
        remote_address: SocketAddress,
        token: stateless_reset::Token,
        max_tag_len: usize,
        triggering_packet_len: usize,
        random_generator: &mut R,
    ) -> Option<Self> {
        let mut packet_buf = [0u8; MINIMUM_MTU as usize];

        let packet_len = packet::stateless_reset::encode_packet(
            token,
            max_tag_len,
            triggering_packet_len,
            random_generator,
            &mut packet_buf,
        )?;

        Some(Self {
            remote_address,
            packet: packet_buf,
            packet_len,
        })
    }
}

impl AsRef<[u8]> for Transmission {
    fn as_ref(&self) -> &[u8] {
        &self.packet[..self.packet_len]
    }
}

impl tx::Message for &Transmission {
    fn remote_address(&mut self) -> SocketAddress {
        self.remote_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    fn delay(&mut self) -> time::Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let packet = self.as_ref();
        buffer[..packet.len()].copy_from_slice(packet);
        packet.len()
    }
}
