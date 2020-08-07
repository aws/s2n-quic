use crate::{
    crypto::{CryptoError, EncryptedPayload, HandshakeCrypto, HeaderCrypto, ProtectedPayload},
    packet::{
        decoding::HeaderDecoder,
        encoding::{PacketEncoder, PacketPayloadEncoder},
        long::{
            DestinationConnectionIDLen, LongPayloadEncoder, LongPayloadLenCursor,
            SourceConnectionIDLen, Version,
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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.4
//# A Handshake packet uses long headers with a type value of 0x2,
//# followed by the Length and Packet Number fields.  The first byte
//# contains the Reserved and Packet Number Length bits.  It is used to
//# carry acknowledgments and cryptographic handshake messages from the
//# server and client.
//#
//# +-+-+-+-+-+-+-+-+
//# |1|1| 2 |R R|P P|
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                         Version (32)                          |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# | DCID Len (8)  |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |               Destination Connection ID (0..160)            ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# | SCID Len (8)  |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                 Source Connection ID (0..160)               ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                           Length (i)                        ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                    Packet Number (8/16/24/32)               ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                          Payload (*)                        ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                 Figure 12: Handshake Protected Packet

macro_rules! handshake_tag {
    () => {
        0b1110u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.4
//# Once a client has received a Handshake packet from a server, it uses
//# Handshake packets to send subsequent cryptographic handshake messages
//# and acknowledgments to the server.
//#
//# The Destination Connection ID field in a Handshake packet contains a
//# connection ID that is chosen by the recipient of the packet; the
//# Source Connection ID includes the connection ID that the sender of
//# the packet wishes to use (see Section 7.2).
//#
//# Handshake packets are their own packet number space, and thus the
//# first Handshake packet sent by a server contains a packet number of
//# 0.
//#
//# The payload of this packet contains CRYPTO frames and could contain
//# PADDING, or ACK frames.  Handshake packets MAY contain
//# CONNECTION_CLOSE frames.  Endpoints MUST treat receipt of Handshake
//# packets with other frames as a connection error.
//#
//# Like Initial packets (see Section 17.2.2.1), data in CRYPTO frames at
//# the Handshake encryption level is discarded - and no longer
//# retransmitted - when Handshake protection keys are discarded.

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
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ProtectedHandshake> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

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

    pub fn unprotect<C: HeaderCrypto>(
        self,
        crypto: &C,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedHandshake<'a>, CryptoError> {
        let Handshake {
            version,
            destination_connection_id,
            source_connection_id,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(crypto, PacketNumberSpace::Handshake, payload)?;

        let packet_number = truncated_packet_number
            .expand(largest_acknowledged_packet_number)
            .ok_or(CryptoError::DECODE_ERROR)?;

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
    pub fn decrypt<C: HandshakeCrypto>(
        self,
        crypto: &C,
    ) -> Result<CleartextHandshake<'a>, CryptoError> {
        let Handshake {
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

impl<'a> CleartextHandshake<'a> {
    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        &self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        &self.source_connection_id
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
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
    }
}

impl<
        DCID: EncoderValue,
        SCID: EncoderValue,
        Payload: PacketPayloadEncoder,
        Crypto: HandshakeCrypto + HeaderCrypto,
    > PacketEncoder<Crypto, Payload> for Handshake<DCID, SCID, PacketNumber, Payload>
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
