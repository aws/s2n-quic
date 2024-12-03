// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{packet_protection, EncryptedPayload, ProtectedPayload, ZeroRttHeaderKey, ZeroRttKey},
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
        Tag,
    },
    varint::VarInt,
};
use s2n_codec::{CheckedRange, DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.3
//# 0-RTT Packet {
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2) = 1,
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

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.3
//# A 0-RTT packet uses long headers with a type value of 0x1,
macro_rules! zero_rtt_tag {
    () => {
        0b1101u8
    };
}

#[derive(Debug)]
pub struct ZeroRtt<DCID, SCID, PacketNumber, Payload> {
    pub version: Version,
    pub destination_connection_id: DCID,
    pub source_connection_id: SCID,
    pub packet_number: PacketNumber,
    pub payload: Payload,
}

pub type ProtectedZeroRtt<'a> =
    ZeroRtt<CheckedRange, CheckedRange, ProtectedPacketNumber, ProtectedPayload<'a>>;
pub type EncryptedZeroRtt<'a> =
    ZeroRtt<CheckedRange, CheckedRange, PacketNumber, EncryptedPayload<'a>>;
pub type CleartextZeroRtt<'a> = ZeroRtt<&'a [u8], &'a [u8], PacketNumber, DecoderBufferMut<'a>>;

impl<'a> ProtectedZeroRtt<'a> {
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ProtectedZeroRtt> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
        //# Endpoints that receive a version 1 long header
        //# with a value larger than 20 MUST drop the packet.
        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;

        let (payload, packet_number, remaining) =
            decoder.finish_long()?.split_off_packet(buffer)?;

        let packet = ZeroRtt {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        };

        Ok((packet, remaining))
    }

    pub fn unprotect<H: ZeroRttHeaderKey>(
        self,
        header_key: &H,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedZeroRtt<'a>, packet_protection::Error> {
        let ZeroRtt {
            version,
            destination_connection_id,
            source_connection_id,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(header_key, PacketNumberSpace::ApplicationData, payload)?;

        let packet_number = truncated_packet_number.expand(largest_acknowledged_packet_number);

        Ok(ZeroRtt {
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

impl<'a> EncryptedZeroRtt<'a> {
    pub fn decrypt<C: ZeroRttKey>(
        self,
        crypto: &C,
    ) -> Result<CleartextZeroRtt<'a>, packet_protection::Error> {
        let ZeroRtt {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        } = self;

        let (header, payload) = crate::crypto::decrypt(crypto, packet_number, payload)?;

        let header = header.into_less_safe_slice();

        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);

        Ok(ZeroRtt {
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

impl CleartextZeroRtt<'_> {
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
    for ZeroRtt<DCID, SCID, TruncatedPacketNumber, Payload>
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
    ZeroRtt<DCID, SCID, PacketNumber, Payload>
{
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        let mut tag: u8 = zero_rtt_tag!() << 4;
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
        K: ZeroRttKey,
        H: ZeroRttHeaderKey,
    > PacketEncoder<K, H, Payload> for ZeroRtt<DCID, SCID, PacketNumber, Payload>
{
    type PayloadLenCursor = LongPayloadLenCursor;

    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        ZeroRtt::encode_header(self, packet_number_len, encoder);
    }

    fn payload(&mut self) -> &mut Payload {
        &mut self.payload
    }
}
