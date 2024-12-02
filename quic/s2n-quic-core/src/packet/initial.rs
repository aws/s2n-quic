// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{packet_protection, EncryptedPayload, InitialHeaderKey, InitialKey, ProtectedPayload},
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
    varint::VarInt,
};
use s2n_codec::{CheckedRange, DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
//# Initial Packet {
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2) = 0,
//#   Reserved Bits (2),
//#   Packet Number Length (2),
//#   Version (32),
//#   Destination Connection ID Length (8),
//#   Destination Connection ID (0..160),
//#   Source Connection ID Length (8),
//#   Source Connection ID (0..160),
//#   Token Length (i),
//#   Token (..),
//#   Length (i),
//#   Packet Number (8..32),
//#   Packet Payload (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
//# An Initial packet uses long headers with a type value of 0x0.
macro_rules! initial_tag {
    () => {
        0b1100u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
//# Token Length:  A variable-length integer specifying the length of the
//# Token field, in bytes.  This value is 0 if no token is present.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
//# Token:  The value of the token that was previously provided in a
//#    Retry packet or NEW_TOKEN frame; see Section 8.1.

#[derive(Debug)]
pub struct Initial<DCID, SCID, Token, PacketNumber, Payload> {
    pub version: Version,
    pub destination_connection_id: DCID,
    pub source_connection_id: SCID,
    pub token: Token,
    pub packet_number: PacketNumber,
    pub payload: Payload,
}

pub type ProtectedInitial<'a> =
    Initial<CheckedRange, CheckedRange, CheckedRange, ProtectedPacketNumber, ProtectedPayload<'a>>;
pub type EncryptedInitial<'a> =
    Initial<CheckedRange, CheckedRange, CheckedRange, PacketNumber, EncryptedPayload<'a>>;
pub type CleartextInitial<'a> =
    Initial<&'a [u8], &'a [u8], &'a [u8], PacketNumber, DecoderBufferMut<'a>>;

impl<'a> ProtectedInitial<'a> {
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ProtectedInitial> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
        //# In order to
        //# properly form a Version Negotiation packet, servers SHOULD be able
        //# to read longer connection IDs from other QUIC versions.
        // Connection ID validation for Initial packets occurs after version
        // negotiation has determined the specified version is supported.
        let destination_connection_id =
            decoder.decode_checked_range::<DestinationConnectionIdLen>(&buffer)?;
        let source_connection_id =
            decoder.decode_checked_range::<SourceConnectionIdLen>(&buffer)?;
        let token = decoder.decode_checked_range::<VarInt>(&buffer)?;

        let (payload, packet_number, remaining) =
            decoder.finish_long()?.split_off_packet(buffer)?;

        let packet = Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
            packet_number,
            payload,
        };

        Ok((packet, remaining))
    }

    pub fn unprotect<H: InitialHeaderKey>(
        self,
        header_key: &H,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedInitial<'a>, packet_protection::Error> {
        let Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(header_key, PacketNumberSpace::Initial, payload)?;

        let packet_number = truncated_packet_number.expand(largest_acknowledged_packet_number);

        Ok(Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
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

    #[inline]
    pub fn token(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.token)
            .into_less_safe_slice()
    }
}

impl<'a> EncryptedInitial<'a> {
    pub fn decrypt<C: InitialKey>(
        self,
        crypto: &C,
    ) -> Result<CleartextInitial<'a>, packet_protection::Error> {
        let Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
            packet_number,
            payload,
        } = self;

        let (header, payload) = crate::crypto::decrypt(crypto, packet_number, payload)?;

        let header = header.into_less_safe_slice();

        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);
        let token = token.get(header);

        Ok(Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
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

    #[inline]
    pub fn token(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.token)
            .into_less_safe_slice()
    }

    // InitialPackets do not have a KeyPhase
    #[inline]
    pub fn key_phase(&self) -> KeyPhase {
        KeyPhase::Zero
    }
}

impl CleartextInitial<'_> {
    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        self.source_connection_id
    }

    #[inline]
    pub fn token(&self) -> &[u8] {
        self.token
    }
}

impl<DCID: EncoderValue, SCID: EncoderValue, Token: EncoderValue, Payload: EncoderValue>
    EncoderValue for Initial<DCID, SCID, Token, TruncatedPacketNumber, Payload>
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

impl<DCID: EncoderValue, SCID: EncoderValue, Token: EncoderValue, PacketNumber, Payload>
    Initial<DCID, SCID, Token, PacketNumber, Payload>
{
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        let mut tag: u8 = initial_tag!() << 4;
        tag |= packet_number_len.into_packet_tag_mask();
        tag.encode(encoder);

        self.version.encode(encoder);

        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIdLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIdLen, E>(encoder);
        self.token.encode_with_len_prefix::<VarInt, E>(encoder);
    }
}

impl<
        DCID: EncoderValue,
        SCID: EncoderValue,
        Token: EncoderValue,
        Payload: PacketPayloadEncoder,
        K: InitialKey,
        H: InitialHeaderKey,
    > PacketEncoder<K, H, Payload> for Initial<DCID, SCID, Token, PacketNumber, Payload>
{
    type PayloadLenCursor = LongPayloadLenCursor;

    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        Initial::encode_header(self, packet_number_len, encoder);
    }

    fn payload(&mut self) -> &mut Payload {
        &mut self.payload
    }
}
