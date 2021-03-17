// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use core::ops::Range;
use s2n_quic_core::{
    connection,
    crypto::RetryKey,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet,
    path::MINIMUM_MTU,
    time, token,
};

#[derive(Debug)]
pub struct Dispatch {
    // TODO: Find a better datastructure capable of handling delays in transmission
    // https://github.com/awslabs/s2n-quic/issues/280
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

    #[allow(dead_code)]
    pub fn queue<T: token::Format, C: RetryKey>(
        &mut self,
        datagram: &DatagramInfo,
        packet: &packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
        token_format: &mut T,
    ) {
        if let Some(transmission) = Transmission::new::<_, C>(
            datagram.remote_address,
            packet,
            local_connection_id,
            token_format,
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
    packet_range: Range<usize>,
}

impl core::fmt::Debug for Transmission {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.remote_address)
            .field("packet", &&self.packet[self.packet_range.clone()])
            .finish()
    }
}

impl Transmission {
    pub fn new<T: token::Format, C: RetryKey>(
        remote_address: SocketAddress,
        packet: &packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
        token_format: &mut T,
    ) -> Option<Self> {
        let mut packet_buf = [0u8; MINIMUM_MTU as usize];
        let packet_range = packet::retry::Retry::encode_packet::<_, C>(
            &remote_address,
            packet,
            &local_connection_id,
            token_format,
            &mut packet_buf,
        )?;

        Some(Self {
            remote_address,
            packet: packet_buf,
            packet_range,
        })
    }
}

impl AsRef<[u8]> for Transmission {
    fn as_ref(&self) -> &[u8] {
        &self.packet[self.packet_range.clone()]
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
