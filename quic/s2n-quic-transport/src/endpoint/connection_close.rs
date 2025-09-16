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
    path, time, transport,
    varint::VarInt,
};

// Size for the Initial packet with the CONNECTION_CLOSE frame in this scenario

// CONNECTION_CLOSE Frame {
//   Type (8) = 0x1c,
//   Error Code (8) = 0x02,
//   Frame Type (8)= 0x00,
//   Reason Phrase Length (8) =0x21,
//   Reason Phrase (264) = "The server refused the connection",
// }

// Initial Packet {
//   Header Form (1) = 1,
//   Fixed Bit (1) = 1,
//   Long Packet Type (2) = 0,
//   Reserved Bits (2),
//   Packet Number Length (2),
//   Version (32),
//   Destination Connection ID Length (8),
//   Destination Connection ID (160), # assuming max length for CID
//   Source Connection ID Length (8),
//   Source Connection ID (160), # assuming max length for CID
//   Token Length (1),
//   Token (0) = no token,
//   Length (12) = length for packet number + payload is 304 bits which needs 12 bits to encode,
//   Packet Number (8),
//   Packet Payload (296) = CONNECTION_CLOSE Frame,
// }

// As shown above, the total size of the initial packet is 693 bits which is 87 bytes.
// We use a slightly larger buffer to ensure the buffer is large enoug to hold the packet.
const DEFAULT_PAYLOAD_SIZE: usize = 150;

#[derive(Debug)]
pub struct Dispatch<Path: path::Handle> {
    transmissions: VecDeque<Transmission<Path>>,
}

impl<Path: path::Handle> Dispatch<Path> {
    pub fn new(max_peers: usize, endpoint_type: endpoint::Type) -> Self {
        // Only the server endpoint can send CONNECTION_CLOSE frame to drop connection request
        let capacity = if endpoint_type.is_server() {
            max_peers
        } else {
            0
        };
        Self {
            transmissions: VecDeque::with_capacity(capacity),
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

                    publisher.on_endpoint_connection_attempt_failed(
                        event::builder::EndpointConnectionAttemptFailed {
                            error: transport::Error::CONNECTION_REFUSED.into(),
                        },
                    );
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
    packet: [u8; DEFAULT_PAYLOAD_SIZE],
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

        let mut packet_buf = [0u8; DEFAULT_PAYLOAD_SIZE];

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
        //# If a server refuses to accept a new connection, it SHOULD send an
        //# Initial packet containing a CONNECTION_CLOSE frame with error code
        //# CONNECTION_REFUSED.

        // We need to ensure that the packet is at least 22 bytes longer than the the minimum connection ID length,
        // that it requests the peer to include in its packets
        // Hewnce, we need to use a reason that's more than 15 bytes to ensure the packet will be sent.
        let connection_close = ConnectionClose {
            error_code: transport::Error::CONNECTION_REFUSED.code.as_varint(),
            frame_type: Some(VarInt::ZERO),
            reason: Some(b"The server refused the connection"),
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
        let encrypted_initial_packet = initial_packet
            .encode_packet(
                &mut initial_key,
                &initial_header_key,
                largest_acknowledged_packet_number,
                None,
                s2n_codec::EncoderBuffer::new(&mut packet_buf),
            )
            .unwrap();

        let packet_range = 0..encrypted_initial_packet.0.len();

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
