// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use s2n_quic_core::{
    event, inet::ExplicitCongestionNotification, io::tx, packet, path,
    path::MINIMUM_MAX_DATAGRAM_SIZE, random, stateless_reset, time,
};

#[derive(Debug)]
pub struct Dispatch<Path: path::Handle> {
    transmissions: VecDeque<Transmission<Path>>,
}

impl<Path: path::Handle> Default for Dispatch<Path> {
    fn default() -> Self {
        Self::new(endpoint::DEFAULT_MAX_PEERS)
    }
}

impl<Path: path::Handle> Dispatch<Path> {
    pub fn new(max_peers: usize) -> Self {
        Self {
            transmissions: VecDeque::with_capacity(max_peers),
        }
    }

    pub fn queue(
        &mut self,
        path: Path,
        token: stateless_reset::Token,
        max_tag_len: usize,
        triggering_packet_len: usize,
        random_generator: &mut dyn random::Generator,
    ) {
        if let Some(transmission) = Transmission::new(
            path,
            token,
            max_tag_len,
            triggering_packet_len,
            random_generator,
        ) {
            self.transmissions.push_back(transmission);
        }
    }

    pub fn on_transmit<Tx: tx::Queue<Handle = Path>, Pub: event::EndpointPublisher>(
        &mut self,
        queue: &mut Tx,
        publisher: &mut Pub,
    ) {
        while let Some(transmission) = self.transmissions.pop_front() {
            match queue.push(&transmission) {
                Ok(tx::Outcome { len, .. }) => {
                    publisher.on_endpoint_packet_sent(event::builder::EndpointPacketSent {
                        packet_header: event::builder::PacketHeader::StatelessReset {},
                    });

                    publisher.on_endpoint_datagram_sent(event::builder::EndpointDatagramSent {
                        len: len as u16,
                        gso_offset: 0,
                    });
                }
                Err(_) => {
                    self.transmissions.push_front(transmission);
                    return;
                }
            }
        }
    }
}

pub struct Transmission<Path: path::Handle> {
    path: Path,
    packet: [u8; MINIMUM_MAX_DATAGRAM_SIZE as usize],
    packet_len: usize,
}

impl<Handle: path::Handle> core::fmt::Debug for Transmission<Handle> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.path.remote_address())
            .field("local_address", &self.path.local_address())
            .field("packet_len", &self.packet_len)
            .field("packet", &&self.packet[0..self.packet_len])
            .finish()
    }
}

impl<Path: path::Handle> Transmission<Path> {
    pub fn new(
        path: Path,
        token: stateless_reset::Token,
        max_tag_len: usize,
        triggering_packet_len: usize,
        random_generator: &mut dyn random::Generator,
    ) -> Option<Self> {
        let mut packet_buf = [0u8; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        let packet_len = packet::stateless_reset::encode_packet(
            token,
            max_tag_len,
            triggering_packet_len,
            random_generator,
            &mut packet_buf,
        )?;

        Some(Self {
            path,
            packet: packet_buf,
            packet_len,
        })
    }
}

impl<Path: path::Handle> AsRef<[u8]> for Transmission<Path> {
    fn as_ref(&self) -> &[u8] {
        &self.packet[..self.packet_len]
    }
}

impl<Path: path::Handle> tx::Message for &Transmission<Path> {
    type Handle = Path;

    #[inline]
    fn path_handle(&self) -> &Self::Handle {
        &self.path
    }

    #[inline]
    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    #[inline]
    fn delay(&mut self) -> time::Duration {
        Default::default()
    }

    #[inline]
    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    #[inline]
    fn can_gso(&self, segment_len: usize, _segment_count: usize) -> bool {
        segment_len >= self.as_ref().len()
    }

    #[inline]
    fn write_payload(
        &mut self,
        mut buffer: tx::PayloadBuffer,
        _gso_offset: usize,
    ) -> Result<usize, tx::Error> {
        buffer.write(self.as_ref())
    }
}
