// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    connection::{id::ConnectionInfo, ProcessingError},
    crypto::{packet_protection, EncryptedPayload, OneRttHeaderKey, OneRttKey, ProtectedPayload},
    packet::{
        decoding::HeaderDecoder,
        encoding::{PacketEncoder, PacketPayloadEncoder},
        number::{
            PacketNumber, PacketNumberLen, PacketNumberSpace, ProtectedPacketNumber,
            TruncatedPacketNumber,
        },
        KeyPhase, ProtectedKeyPhase, Tag,
    },
    transport,
};
use s2n_codec::{CheckedRange, DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# 1-RTT Packet {
//#   Header Form (1) = 0,
//#   Fixed Bit (1) = 1,
//#   Spin Bit (1),
//#   Reserved Bits (2),
//#   Key Phase (1),
//#   Packet Number Length (2),
//#   Destination Connection ID (0..160),
//#   Packet Number (8..32),
//#   Packet Payload (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Header Form:  The most significant bit (0x80) of byte 0 is set to 0
//#    for the short header.
//#
//# Fixed Bit:  The next bit (0x40) of byte 0 is set to 1.

macro_rules! short_tag {
    () => {
        0b0100u8..=0b0111u8
    };
}

const ENCODING_TAG: u8 = 0b0100_0000;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Spin Bit:  The third most significant bit (0x20) of byte 0 is the
//#    latency spin bit, set as described in Section 17.4.

const SPIN_BIT_MASK: u8 = 0x20;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//#  Reserved Bits:  The next two bits (those with a mask of 0x18) of byte
//#      0 are reserved.

const RESERVED_BITS_MASK: u8 = 0x18;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpinBit {
    Zero,
    One,
}

impl Default for SpinBit {
    fn default() -> Self {
        Self::Zero
    }
}

impl SpinBit {
    fn from_tag(tag: Tag) -> Self {
        if tag & SPIN_BIT_MASK == SPIN_BIT_MASK {
            Self::One
        } else {
            Self::Zero
        }
    }

    fn into_packet_tag_mask(self) -> u8 {
        match self {
            Self::One => SPIN_BIT_MASK,
            Self::Zero => 0,
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Reserved Bits:  The next two bits (those with a mask of 0x18) of byte
//#    0 are reserved.  These bits are protected using header protection;
//#    see Section 5.4 of [QUIC-TLS].

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Packet Number Length:  The least significant two bits (those with a
//#    mask of 0x03) of byte 0 contain the length of the Packet Number
//#    field, encoded as an unsigned two-bit integer that is one less
//#    than the length of the Packet Number field in bytes.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Destination Connection ID:  The Destination Connection ID is a
//#    connection ID that is chosen by the intended recipient of the
//#    packet.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Packet Number:  The Packet Number field is 1 to 4 bytes long.  The
//#    packet number is protected using header protection; see
//#    Section 5.4 of [QUIC-TLS].  The length of the Packet Number field
//#    is encoded in Packet Number Length field.  See Section 17.1 for
//#    details.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
//# Packet Payload:  1-RTT packets always include a 1-RTT protected
//#    payload.

#[derive(Debug)]
pub struct Short<DCID, KeyPhase, PacketNumber, Payload> {
    pub spin_bit: SpinBit,
    pub key_phase: KeyPhase,
    pub destination_connection_id: DCID,
    pub packet_number: PacketNumber,
    pub payload: Payload,
}

pub type ProtectedShort<'a> =
    Short<CheckedRange, ProtectedKeyPhase, ProtectedPacketNumber, ProtectedPayload<'a>>;
pub type EncryptedShort<'a> = Short<CheckedRange, KeyPhase, PacketNumber, EncryptedPayload<'a>>;
pub type CleartextShort<'a> = Short<&'a [u8], KeyPhase, PacketNumber, DecoderBufferMut<'a>>;

impl<'a> ProtectedShort<'a> {
    #[inline]
    pub(crate) fn decode<Validator: connection::id::Validator>(
        tag: Tag,
        buffer: DecoderBufferMut<'a>,
        connection_info: &ConnectionInfo,
        destination_connection_id_decoder: &Validator,
    ) -> DecoderBufferMutResult<'a, ProtectedShort<'a>> {
        let mut decoder = HeaderDecoder::new_short(&buffer);

        let spin_bit = SpinBit::from_tag(tag);
        let key_phase = ProtectedKeyPhase;

        let destination_connection_id = decoder.decode_short_destination_connection_id(
            &buffer,
            connection_info,
            destination_connection_id_decoder,
        )?;

        let (payload, packet_number, remaining) =
            decoder.finish_short()?.split_off_packet(buffer)?;

        let packet = Short {
            spin_bit,
            key_phase,
            destination_connection_id,
            packet_number,
            payload,
        };

        Ok((packet, remaining))
    }

    pub fn unprotect<H: OneRttHeaderKey>(
        self,
        header_key: &H,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedShort<'a>, packet_protection::Error> {
        let Short {
            spin_bit,
            destination_connection_id,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(header_key, PacketNumberSpace::ApplicationData, payload)?;

        let key_phase = KeyPhase::from_tag(payload.get_tag());

        let packet_number = truncated_packet_number.expand(largest_acknowledged_packet_number);

        Ok(Short {
            spin_bit,
            key_phase,
            destination_connection_id,
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

    pub fn get_wire_bytes(&self) -> Vec<u8> {
        self.payload.buffer.encode_to_vec()
    }
}

impl<'a> EncryptedShort<'a> {
    pub fn decrypt<C: OneRttKey>(self, crypto: &C) -> Result<CleartextShort<'a>, ProcessingError> {
        let Short {
            spin_bit,
            key_phase,
            destination_connection_id,
            packet_number,
            payload,
        } = self;

        let (header, payload) = crate::crypto::decrypt(crypto, packet_number, payload)?;

        let header = header.into_less_safe_slice();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.3.1
        //# An endpoint MUST treat receipt of a
        //# packet that has a non-zero value for these bits, after removing
        //# both packet and header protection, as a connection error of type
        //# PROTOCOL_VIOLATION.
        if header[0] & RESERVED_BITS_MASK != 0 {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("reserved bits are non-zero")
                .into());
        }

        let destination_connection_id = destination_connection_id.get(header);

        Ok(Short {
            spin_bit,
            key_phase,
            destination_connection_id,
            packet_number,
            payload,
        })
    }

    #[inline]
    pub fn key_phase(&self) -> KeyPhase {
        self.key_phase
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.payload
            .get_checked_range(&self.destination_connection_id)
            .into_less_safe_slice()
    }
}

impl CleartextShort<'_> {
    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.destination_connection_id
    }
}

impl<DCID: EncoderValue, Payload: EncoderValue> EncoderValue
    for Short<DCID, KeyPhase, TruncatedPacketNumber, Payload>
{
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.encode_header(self.packet_number.len(), encoder);
        self.packet_number.encode(encoder);
        self.payload.encode(encoder);
    }
}

impl<DCID: EncoderValue, PacketNumber, Payload> Short<DCID, KeyPhase, PacketNumber, Payload> {
    #[inline]
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        (ENCODING_TAG
            | self.spin_bit.into_packet_tag_mask()
            | self.key_phase.into_packet_tag_mask()
            | packet_number_len.into_packet_tag_mask())
        .encode(encoder);

        self.destination_connection_id.encode(encoder);
    }
}

impl<DCID: EncoderValue, Payload: PacketPayloadEncoder, K: OneRttKey, H: OneRttHeaderKey>
    PacketEncoder<K, H, Payload> for Short<DCID, KeyPhase, PacketNumber, Payload>
{
    type PayloadLenCursor = ();

    #[inline]
    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        Short::encode_header(self, packet_number_len, encoder);
    }

    #[inline]
    fn payload(&mut self) -> &mut Payload {
        &mut self.payload
    }
}
