// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint;
use alloc::collections::VecDeque;
use core::ops::Range;
use s2n_codec::{DecoderBufferMut, EncoderValue};
use s2n_quic_core::{
    connection,
    crypto::{InitialHeaderKey, InitialKey},
    event,
    frame::ConnectionClose,
    inet::ExplicitCongestionNotification,
    io::tx,
    packet::{initial::CleartextInitial, number::PacketNumberSpace},
    path::{self, MINIMUM_MAX_DATAGRAM_SIZE},
    time, transport,
    varint::VarInt,
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

    pub fn queue<C: InitialKey>(
        &mut self,
        path_handle: Path,
        packet: &s2n_quic_core::packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
    ) where
        <C as InitialKey>::HeaderKey: InitialHeaderKey,
    {
        if let Some(transmission) = Transmission::new::<C>(path_handle, packet, local_connection_id)
        {
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
                        packet_header: event::builder::PacketHeader::Initial {
                            number: transmission.packet_number,
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
    packet_number: u64,
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
    pub fn new<C: InitialKey>(
        path: Path,
        packet: &s2n_quic_core::packet::initial::ProtectedInitial,
        local_connection_id: connection::LocalId,
    ) -> Option<Self>
    where
        <C as InitialKey>::HeaderKey: InitialHeaderKey,
    {
        use s2n_quic_core::packet::encoding::PacketEncoder;
        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.1
        //# This value MUST NOT be equal to the Destination
        //# Connection ID field of the packet sent by the client.
        debug_assert_ne!(
            local_connection_id.as_ref(),
            packet.destination_connection_id()
        );
        if local_connection_id.as_ref() == packet.destination_connection_id() {
            return None;
        }

        let mut packet_buf = [0u8; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        // Create a connection close frame with CONNECTION_REFUSED error code
        let connection_close = ConnectionClose {
            error_code: transport::Error::CONNECTION_REFUSED.code.as_varint(),
            frame_type: Some(VarInt::ZERO),
            reason: Some(b"The server's limiter refused the connection"),
        };

        let mut encoded_frame = connection_close.encode_to_vec();

        // Generate a new packet number for the connection close initial packet
        let packet_number = PacketNumberSpace::Initial.new_packet_number(Default::default());

        // Create an initial packet with the connection close frame
        let initial_packet = CleartextInitial {
            version: packet.version,
            destination_connection_id: packet.source_connection_id(),
            source_connection_id: local_connection_id.as_bytes(),
            token: &[],
            packet_number,
            payload: DecoderBufferMut::new(&mut encoded_frame),
        };

        let (mut initial_key, initial_header_key) =
            C::new_server(packet.destination_connection_id());

        // There is no packet acknowledged yet, since no packet is taken from the peer.
        // The endpoint just close the connection immediately.
        let largest_acknowledged_packet_number =
            PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);

        // Use the PacketEncoder trait to encode, encrypt, and protect the packet
        let packet_range = match initial_packet.encode_packet(
            &mut initial_key,
            &initial_header_key,
            largest_acknowledged_packet_number,
            None,
            s2n_codec::EncoderBuffer::new(&mut packet_buf),
        ) {
            Ok((protected_payload, _)) => 0..protected_payload.len(),
            Err(e) => {
                println!("The error is {:?}", e);
                return None;
            }
        };

        Some(Self {
            path,
            packet: packet_buf,
            packet_range,
            version: packet.version,
            packet_number: packet_number.as_u64(),
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
