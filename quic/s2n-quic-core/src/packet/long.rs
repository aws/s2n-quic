use crate::{
    packet::{encoding::PacketPayloadLenCursor, number::TruncatedPacketNumber},
    varint::VarInt,
};
use core::convert::TryFrom;
use s2n_codec::{
    decoder_invariant, CheckedRange, DecoderError, Encoder, EncoderBuffer, EncoderValue,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2
//# 17.2.  Long Header Packets
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+
//#    |1|1|T T|X X X X|
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         Version (32)                          |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    | DCID Len (8)  |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |               Destination Connection ID (0..160)            ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    | SCID Len (8)  |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                 Source Connection ID (0..160)               ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                     Figure 9: Long Header Packet Format
//#
//#    Long headers are used for packets that are sent prior to the
//#    establishment of 1-RTT keys.  Once both conditions are met, a sender
//#    switches to sending packets using the short header (Section 17.3).
//#    The long form allows for special packets - such as the Version
//#    Negotiation packet - to be represented in this uniform fixed-length
//#    packet format.  Packets that use the long header contain the
//#    following fields:
//#
//#    Header Form:  The most significant bit (0x80) of byte 0 (the first
//#      byte) is set to 1 for long headers.
//#
//#    Fixed Bit:  The next bit (0x40) of byte 0 is set to 1.  Packets
//#       containing a zero value for this bit are not valid packets in this
//#       version and MUST be discarded.
//#
//#    Long Packet Type (T):  The next two bits (those with a mask of 0x30)
//#       of byte 0 contain a packet type.  Packet types are listed in
//#       Table 5.

pub(crate) const PACKET_TYPE_MASK: u8 = 0x30;
const PACKET_TYPE_OFFSET: u8 = 4;

//#    Type-Specific Bits (X):  The lower four bits (those with a mask of
//#       0x0f) of byte 0 are type-specific.
//#
//#    Version:  The QUIC Version is a 32-bit field that follows the first
//#       byte.  This field indicates which version of QUIC is in use and
//#       determines how the rest of the protocol fields are interpreted.

pub(crate) type Version = u32;

//#    DCID Len:  The byte following the version contains the length in
//#       bytes of the Destination Connection ID field that follows it.
//#       This length is encoded as an 8-bit unsigned integer.  In QUIC
//#       version 1, this value MUST NOT exceed 20.  Endpoints that receive
//#       a version 1 long header with a value larger than 20 MUST drop the
//#       packet.  Servers SHOULD be able to read longer connection IDs from
//#       other QUIC versions in order to properly form a version
//#       negotiation packet.

pub(crate) type DestinationConnectionIDLen = u8;
pub(crate) const DESTINATION_CONNECTION_ID_MAX_LEN: usize = 20;

//#    Destination Connection ID:  The Destination Connection ID field
//#      follows the DCID Len and is between 0 and 20 bytes in length.
//#      Section 7.2 describes the use of this field in more detail.

pub(crate) fn validate_destination_connection_id_range(
    range: &CheckedRange,
) -> Result<(), DecoderError> {
    validate_destination_connection_id_len(range.len())
}

pub(crate) fn validate_destination_connection_id_len(len: usize) -> Result<(), DecoderError> {
    decoder_invariant!(
        len <= DESTINATION_CONNECTION_ID_MAX_LEN,
        "destination connection exceeds max length"
    );
    Ok(())
}

//#    SCID Len:  The byte following the Destination Connection ID contains
//#      the length in bytes of the Source Connection ID field that follows
//#      it.  This length is encoded as a 8-bit unsigned integer.  In QUIC
//#      version 1, this value MUST NOT exceed 20 bytes.  Endpoints that
//#      receive a version 1 long header with a value larger than 20 MUST
//#      drop the packet.  Servers SHOULD be able to read longer connection
//#      IDs from other QUIC versions in order to properly form a version
//#      negotiation packet.

pub(crate) type SourceConnectionIDLen = u8;
pub(crate) const SOURCE_CONNECTION_ID_MAX_LEN: usize = 20;

//#    Source Connection ID:  The Source Connection ID field follows the
//#      SCID Len and is between 0 and 20 bytes in length.  Section 7.2
//#      describes the use of this field in more detail.

pub(crate) fn validate_source_connection_id_range(
    range: &CheckedRange,
) -> Result<(), DecoderError> {
    validate_source_connection_id_len(range.len())
}

pub(crate) fn validate_source_connection_id_len(len: usize) -> Result<(), DecoderError> {
    decoder_invariant!(
        len <= SOURCE_CONNECTION_ID_MAX_LEN,
        "source connection exceeds max length"
    );
    Ok(())
}

//#    In this version of QUIC, the following packet types with the long
//#    header are defined:
//#
//#                    +------+-----------+----------------+
//#                    | Type | Name      | Section        |
//#                    +------+-----------+----------------+
//#                    |  0x0 | Initial   | Section 17.2.2 |
//#                    |      |           |                |
//#                    |  0x1 | 0-RTT     | Section 17.2.3 |
//#                    |      |           |                |
//#                    |  0x2 | Handshake | Section 17.2.4 |
//#                    |      |           |                |
//#                    |  0x3 | Retry     | Section 17.2.5 |
//#                    +------+-----------+----------------+
//#
//#                      Table 5: Long Header Packet Types

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum PacketType {
    Initial = 0x0,
    ZeroRTT = 0x1,
    Handshake = 0x2,
    Retry = 0x3,
}

impl PacketType {
    pub const fn into_bits(self) -> u8 {
        (self as u8) << PACKET_TYPE_OFFSET & PACKET_TYPE_MASK
    }

    pub fn from_bits(bits: u8) -> Self {
        (bits & PACKET_TYPE_MASK >> PACKET_TYPE_OFFSET).into()
    }
}

impl From<u8> for PacketType {
    fn from(bits: u8) -> Self {
        match bits {
            0x0 => PacketType::Initial,
            0x1 => PacketType::ZeroRTT,
            0x2 => PacketType::Handshake,
            0x3 => PacketType::Retry,
            _ => PacketType::Initial,
        }
    }
}

impl Into<u8> for PacketType {
    fn into(self) -> u8 {
        self.into_bits()
    }
}

//#    The header form bit, connection ID lengths byte, Destination and
//#    Source Connection ID fields, and Version fields of a long header
//#    packet are version-independent.  The other fields in the first byte
//#    are version-specific.  See [QUIC-INVARIANTS] for details on how
//#    packets from different versions of QUIC are interpreted.
//#
//#    The interpretation of the fields and the payload are specific to a
//#    version and packet type.  While type-specific semantics for this
//#    version are described in the following sections, several long-header
//#    packets in this version of QUIC contain these additional fields:
//#
//#    Reserved Bits (R):  Two bits (those with a mask of 0x0c) of byte 0
//#       are reserved across multiple packet types.  These bits are
//#       protected using header protection (see Section 5.4 of [QUIC-TLS]).
//#       The value included prior to protection MUST be set to 0.  An
//#       endpoint MUST treat receipt of a packet that has a non-zero value
//#       for these bits, after removing both packet and header protection,
//#       as a connection error of type PROTOCOL_VIOLATION.  Discarding such
//#       a packet after only removing header protection can expose the
//#       endpoint to attacks (see Section 9.3 of [QUIC-TLS]).
//#
//#    Packet Number Length (P):  In packet types which contain a Packet
//#       Number field, the least significant two bits (those with a mask of
//#       0x03) of byte 0 contain the length of the packet number, encoded
//#       as an unsigned, two-bit integer that is one less than the length
//#       of the packet number field in bytes.  That is, the length of the
//#       packet number field is the value of this field, plus one.  These
//#       bits are protected using header protection (see Section 5.4 of
//#       [QUIC-TLS]).
//#
//#    Length:  The length of the remainder of the packet (that is, the
//#       Packet Number and Payload fields) in bytes, encoded as a variable-
//#       length integer (Section 16).
//#
//#    Packet Number:  The packet number field is 1 to 4 bytes long.  The
//#       packet number has confidentiality protection separate from packet
//#       protection, as described in Section 5.4 of [QUIC-TLS].  The length
//#       of the packet number field is encoded in the Packet Number Length
//#       bits of byte 0 (see above).

pub(crate) struct LongPayloadEncoder<Payload> {
    pub packet_number: TruncatedPacketNumber,
    pub payload: Payload,
}

impl<'a, Payload: EncoderValue> EncoderValue for LongPayloadEncoder<&'a Payload> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.packet_number.encode(encoder);
        self.payload.encode(encoder);
    }
}

impl<'a, Payload: EncoderValue> EncoderValue for LongPayloadEncoder<&'a mut Payload> {
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
