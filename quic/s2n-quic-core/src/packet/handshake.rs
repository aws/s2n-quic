// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::ProcessingError,
    crypto::{
        packet_protection, EncryptedPayload, HandshakeHeaderKey, HandshakeKey, ProtectedPayload,
    },
    packet::{
        decoding::HeaderDecoder,
        encoding::{PacketEncoder, PacketPayloadEncoder},
        long::{
            DestinationConnectionIdLen, LongPayloadEncoder, LongPayloadLenCursor,
            SourceConnectionIdLen, Version,
        },
        number::{
            PacketNumber, PacketNumberLen, PacketNumberSpace, ProtectedPacketNumber,
            TruncatedPacketNumber,
        },
        KeyPhase, Tag,
    },
    transport,
    varint::VarInt,
};
use s2n_codec::{CheckedRange, DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.4
//# Handshake Packet {
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2) = 2,
//#   Reserved Bits (2),
//#   Packet Number Length (2),
//#   Version (32),
//#   Destination Connection ID Length (8),
//#   Destination Connection ID (0..160),
//#   Source Connection ID Length (8),
//#   Source Connection ID (0..160),
//#   Length (i),
//#   Packet Number (8..32),
//#   Packet Payload (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.4
//# A Handshake packet uses long headers with a type value of 0x2
macro_rules! handshake_tag {
    () => {
        0b1110u8
    };
}

#[derive(Debug)]
pub struct Handshake<DCID, SCID, PacketNumber, Payload> {
    pub version: Version,
    pub destination_connection_id: DCID,
    pub source_connection_id: SCID,
    pub packet_number: PacketNumber,
    pub payload: Payload,
}

pub type ProtectedHandshake<'a> =
    Handshake<CheckedRange, CheckedRange, ProtectedPacketNumber, ProtectedPayload<'a>>;
pub type EncryptedHandshake<'a> =
    Handshake<CheckedRange, CheckedRange, PacketNumber, EncryptedPayload<'a>>;
pub type CleartextHandshake<'a> = Handshake<&'a [u8], &'a [u8], PacketNumber, DecoderBufferMut<'a>>;

impl<'a> ProtectedHandshake<'a> {
    pub fn get_wire_bytes(&self) -> Vec<u8> {
        self.payload.buffer.encode_to_vec()
    }

    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ProtectedHandshake> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
        //# Endpoints that receive a version 1 long header
        //# with a value larger than 20 MUST drop the packet.
        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;

        let (payload, packet_number, remaining) =
            decoder.finish_long()?.split_off_packet(buffer)?;

        let packet = Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        };

        Ok((packet, remaining))
    }

    pub fn unprotect<K: HandshakeHeaderKey>(
        self,
        key: &K,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedHandshake<'a>, packet_protection::Error> {
        let Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(key, PacketNumberSpace::Handshake, payload)?;

        let packet_number = truncated_packet_number.expand(largest_acknowledged_packet_number);

        Ok(Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        })
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.destination_connection_id)
            .into_less_safe_slice()
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.source_connection_id)
            .into_less_safe_slice()
    }
}

impl<'a> EncryptedHandshake<'a> {
    pub fn decrypt<C: HandshakeKey>(
        self,
        crypto: &C,
    ) -> Result<CleartextHandshake<'a>, ProcessingError> {
        let Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        } = self;

        let (header, payload) = crate::crypto::decrypt(crypto, packet_number, payload)?;

        let header = header.into_less_safe_slice();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
        //# The value
        //# included prior to protection MUST be set to 0.  An endpoint MUST
        //# treat receipt of a packet that has a non-zero value for these bits
        //# after removing both packet and header protection as a connection
        //# error of type PROTOCOL_VIOLATION.
        if header[0] & super::long::RESERVED_BITS_MASK != 0 {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("reserved bits are non-zero")
                .into());
        }

        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);

        Ok(Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        })
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.destination_connection_id)
            .into_less_safe_slice()
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.source_connection_id)
            .into_less_safe_slice()
    }

    // HandshakePackets do not have a KeyPhase
    #[inline]
    pub fn key_phase(&self) -> KeyPhase {
        KeyPhase::Zero
    }
}

impl CleartextHandshake<'_> {
    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        self.source_connection_id
    }
}

impl<DCID: EncoderValue, SCID: EncoderValue, Payload: EncoderValue> EncoderValue
    for Handshake<DCID, SCID, TruncatedPacketNumber, Payload>
{
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.encode_header(self.packet_number.len(), encoder);
        LongPayloadEncoder {
            packet_number: self.packet_number,
            payload: &self.payload,
        }
        .encode_with_len_prefix::<VarInt, E>(encoder)
    }
}

impl<DCID: EncoderValue, SCID: EncoderValue, PacketNumber, Payload>
    Handshake<DCID, SCID, PacketNumber, Payload>
{
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        let mut tag: u8 = handshake_tag!() << 4;
        tag |= packet_number_len.into_packet_tag_mask();
        tag.encode(encoder);

        self.version.encode(encoder);
        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIdLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIdLen, E>(encoder);
    }
}

impl<
        DCID: EncoderValue,
        SCID: EncoderValue,
        Payload: PacketPayloadEncoder,
        K: HandshakeKey,
        H: HandshakeHeaderKey,
    > PacketEncoder<K, H, Payload> for Handshake<DCID, SCID, PacketNumber, Payload>
{
    type PayloadLenCursor = LongPayloadLenCursor;

    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        Handshake::encode_header(self, packet_number_len, encoder);
    }

    fn payload(&mut self) -> &mut Payload {
        &mut self.payload
    }
}
