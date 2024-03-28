// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use core::ops::Range;
use s2n_quic_core::{
    connection,
    crypto::RetryKey,
    event,
    inet::ExplicitCongestionNotification,
    io::tx,
    packet,
    path::{self, MINIMUM_MAX_DATAGRAM_SIZE},
    random, time, token,
};

#[derive(Debug)]
pub struct Dispatch<Path: path::Handle> {
    // TODO: Find a better datastructure capable of handling delays in transmission
    // https://github.com/aws/s2n-quic/issues/280
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

    pub fn queue<T: token::Format, C: RetryKey>(
        &mut self,
        path_handle: Path,
        packet: &packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
        random: &mut dyn random::Generator,
        token_format: &mut T,
    ) {
        if let Some(transmission) = Transmission::new::<_, C>(
            path_handle,
            packet,
            local_connection_id,
            random,
            token_format,
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
                        packet_header: event::builder::PacketHeader::Retry {
                            version: transmission.version,
                        },
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
    packet_range: Range<usize>,
    version: u32,
}

impl<Path: path::Handle> core::fmt::Debug for Transmission<Path> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Transmission")
            .field("remote_address", &self.path.remote_address())
            .field("local_address", &self.path.local_address())
            .field("packet", &&self.packet[self.packet_range.clone()])
            .finish()
    }
}

impl<Path: path::Handle> Transmission<Path> {
    pub fn new<T: token::Format, C: RetryKey>(
        path: Path,
        packet: &packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
        random: &mut dyn random::Generator,
        token_format: &mut T,
    ) -> Option<Self> {
        let mut packet_buf = [0u8; MINIMUM_MAX_DATAGRAM_SIZE as usize];
        let packet_range = packet::retry::Retry::encode_packet::<_, C>(
            &path.remote_address(),
            packet,
            &local_connection_id,
            random,
            token_format,
            &mut packet_buf,
        )?;

        Some(Self {
            path,
            packet: packet_buf,
            packet_range,
            version: packet.version,
        })
    }
}

impl<Path: path::Handle> AsRef<[u8]> for Transmission<Path> {
    fn as_ref(&self) -> &[u8] {
        &self.packet[self.packet_range.clone()]
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
