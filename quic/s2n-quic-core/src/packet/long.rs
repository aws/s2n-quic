// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::{encoding::PacketPayloadLenCursor, number::TruncatedPacketNumber},
    varint::VarInt,
};
use s2n_codec::{
    decoder_invariant, CheckedRange, DecoderError, Encoder, EncoderBuffer, EncoderValue,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Long Header Packet {
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2),
//#   Type-Specific Bits (4),
//#   Version (32),
//#   Destination Connection ID Length (8),
//#   Destination Connection ID (0..160),
//#   Source Connection ID Length (8),
//#   Source Connection ID (0..160),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Header Form:  The most significant bit (0x80) of byte 0 (the first
//#   byte) is set to 1 for long headers.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Fixed Bit:  The next bit (0x40) of byte 0 is set to 1.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Long Packet Type:  The next two bits (those with a mask of 0x30)
//#    of byte 0 contain a packet type.  Packet types are listed in
//#    Table 5.

pub(crate) const PACKET_TYPE_MASK: u8 = 0x30;
const PACKET_TYPE_OFFSET: u8 = 4;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Type-Specific Bits:  The semantics of the lower four bits (those with
//# a mask of 0x0f) of byte 0 are determined by the packet type.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Version: The QUIC Version is a 32-bit field that follows the first
//#    byte.  This field indicates the version of QUIC that is in use and
//#    determines how the rest of the protocol fields are interpreted.

pub(crate) type Version = u32;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Destination Connection ID Length:  The byte following the version
//#    contains the length in bytes of the Destination Connection ID
//#    field that follows it.  This length is encoded as an 8-bit
//#    unsigned integer.  In QUIC version 1, this value MUST NOT exceed
//#    20.

pub(crate) type DestinationConnectionIdLen = u8;
pub(crate) const DESTINATION_CONNECTION_ID_MAX_LEN: usize = 20;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Destination Connection ID:  The Destination Connection ID field
//#   follows the Destination Connection ID Length field, which
//#   indicates the length of this field.  Section 7.2 describes the use
//#   of this field in more detail.

pub(crate) fn validate_destination_connection_id_range(
    range: &CheckedRange,
) -> Result<(), DecoderError> {
    validate_destination_connection_id_len(range.len())
}

pub(crate) fn validate_destination_connection_id_len(len: usize) -> Result<(), DecoderError> {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
    //# Endpoints that receive a version 1 long header with a value
    //# larger than 20 MUST drop the packet.
    decoder_invariant!(
        len <= DESTINATION_CONNECTION_ID_MAX_LEN,
        "destination connection exceeds max length"
    );
    Ok(())
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Source Connection ID Length:  The byte following the Destination
//#   Connection ID contains the length in bytes of the Source
//#   Connection ID field that follows it.  This length is encoded as a
//#   8-bit unsigned integer.  In QUIC version 1, this value MUST NOT
//#   exceed 20 bytes.

pub(crate) type SourceConnectionIdLen = u8;
pub(crate) const SOURCE_CONNECTION_ID_MAX_LEN: usize = 20;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Source Connection ID:  The Source Connection ID field follows the
//#   Source Connection ID Length field, which indicates the length of
//#   this field.  Section 7.2 describes the use of this field in more
//#   detail.

pub(crate) fn validate_source_connection_id_range(
    range: &CheckedRange,
) -> Result<(), DecoderError> {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
    //# Endpoints that receive a version 1 long header
    //# with a value larger than 20 MUST drop the packet.
    validate_source_connection_id_len(range.len())
}

pub(crate) fn validate_source_connection_id_len(len: usize) -> Result<(), DecoderError> {
    decoder_invariant!(
        len <= SOURCE_CONNECTION_ID_MAX_LEN,
        "source connection exceeds max length"
    );
    Ok(())
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# In this version of QUIC, the following packet types with the long
//# header are defined:
//#
//#                 +======+===========+================+
//#                 | Type | Name      | Section        |
//#                 +======+===========+================+
//#                 | 0x00 | Initial   | Section 17.2.2 |
//#                 +------+-----------+----------------+
//#                 | 0x01 | 0-RTT     | Section 17.2.3 |
//#                 +------+-----------+----------------+
//#                 | 0x02 | Handshake | Section 17.2.4 |
//#                 +------+-----------+----------------+
//#                 | 0x03 | Retry     | Section 17.2.5 |
//#                 +------+-----------+----------------+
//#
//#                   Table 5: Long Header Packet Types

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum PacketType {
    Initial = 0x0,
    ZeroRtt = 0x1,
    Handshake = 0x2,
    Retry = 0x3,
}

impl PacketType {
    pub const fn into_bits(self) -> u8 {
        ((self as u8) << PACKET_TYPE_OFFSET) & PACKET_TYPE_MASK
    }

    pub fn from_bits(bits: u8) -> Self {
        (bits & (PACKET_TYPE_MASK >> PACKET_TYPE_OFFSET)).into()
    }
}

impl From<u8> for PacketType {
    fn from(bits: u8) -> Self {
        match bits {
            0x0 => PacketType::Initial,
            0x1 => PacketType::ZeroRtt,
            0x2 => PacketType::Handshake,
            0x3 => PacketType::Retry,
            _ => PacketType::Initial,
        }
    }
}

impl From<PacketType> for u8 {
    fn from(v: PacketType) -> Self {
        v.into_bits()
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Reserved Bits:  Two bits (those with a mask of 0x0c) of byte 0 are
//#    reserved across multiple packet types.  These bits are protected
//#    using header protection; see Section 5.4 of [QUIC-TLS].
pub const RESERVED_BITS_MASK: u8 = 0x0c;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Packet Number Length:  In packet types that contain a Packet Number
//# field, the least significant two bits (those with a mask of 0x03)
//# of byte 0 contain the length of the Packet Number field, encoded
//# as an unsigned two-bit integer that is one less than the length of
//# the Packet Number field in bytes.

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Length:  This is the length of the remainder of the packet (that is,
//# the Packet Number and Payload fields) in bytes, encoded as a
//# variable-length integer (Section 16).

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2
//# Packet Number:  This field is 1 to 4 bytes long.  The packet number
//# is protected using header protection; see Section 5.4 of
//# [QUIC-TLS].  The length of the Packet Number field is encoded in
//# the Packet Number Length bits of byte 0; see above.

pub(crate) struct LongPayloadEncoder<Payload> {
    pub packet_number: TruncatedPacketNumber,
    pub payload: Payload,
}

impl<Payload: EncoderValue> EncoderValue for LongPayloadEncoder<&Payload> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.packet_number.encode(encoder);
        self.payload.encode(encoder);
    }
}

impl<Payload: EncoderValue> EncoderValue for LongPayloadEncoder<&mut Payload> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.packet_number.encode(encoder);
        self.payload.encode(encoder);
    }

    fn encode_mut<E: Encoder>(&mut self, encoder: &mut E) {
        self.packet_number.encode_mut(encoder);
        self.payload.encode_mut(encoder);
    }
}

// used internally for estimating long packet payload len values.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LongPayloadLenCursor {
    position: usize,
    max_value: VarInt,
}

impl PacketPayloadLenCursor for LongPayloadLenCursor {
    fn new() -> Self {
        let max_value = VarInt::MAX;
        Self {
            position: 0,
            max_value,
        }
    }

    fn update(&self, buffer: &mut EncoderBuffer, actual_len: usize) {
        debug_assert!(
            self.position != 0,
            "position cursor was not updated. encode_mut should be called instead of encode"
        );

        let actual_value =
            VarInt::try_from(actual_len).expect("packets should not be larger than VarInt::MAX");
        let max_value = self.max_value;

        let prev_pos = buffer.len();
        buffer.set_position(self.position);
        max_value.encode_updated(actual_value, buffer);
        buffer.set_position(prev_pos);
    }
}

impl EncoderValue for LongPayloadLenCursor {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.max_value.encode(encoder)
    }

    fn encode_mut<E: Encoder>(&mut self, encoder: &mut E) {
        self.position = encoder.len();
        self.max_value = VarInt::try_from(encoder.remaining_capacity()).unwrap_or(VarInt::MAX);
        self.max_value.encode(encoder)
    }
}
