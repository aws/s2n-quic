use crate::{
    crypto::{CryptoError, EncryptedPayload, HeaderCrypto, ProtectedPayload, ZeroRTTCrypto},
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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.3
//# A 0-RTT packet uses long headers with a type value of 0x1, followed
//# by the Length and Packet Number fields.  The first byte contains the
//# Reserved and Packet Number Length bits.  It is used to carry "early"
//# data from the client to the server as part of the first flight, prior
//# to handshake completion.  As part of the TLS handshake, the server
//# can accept or reject this early data.
//#
//# See Section 2.3 of [TLS13] for a discussion of 0-RTT data and its
//# limitations.
//#
//# +-+-+-+-+-+-+-+-+
//# |1|1| 1 |R R|P P|
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
//#                             0-RTT Packet

macro_rules! zero_rtt_tag {
    () => {
        0b1101u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.3
//# Packet numbers for 0-RTT protected packets use the same space as
//# 1-RTT protected packets.
//#
//# After a client receives a Retry packet, 0-RTT packets are likely to
//# have been lost or discarded by the server.  A client MAY attempt to
//# resend data in 0-RTT packets after it sends a new Initial packet.
//#
//# A client MUST NOT reset the packet number it uses for 0-RTT packets,
//# since the keys used to protect 0-RTT packets will not change as a
//# result of responding to a Retry packet.  Sending packets with the
//# same packet number in that case is likely to compromise the packet
//# protection for all 0-RTT packets because the same key and nonce could
//# be used to protect different content.
//#
//# A client only receives acknowledgments for its 0-RTT packets once the
//# handshake is complete.  Consequently, a server might expect 0-RTT
//# packets to start with a packet number of 0.  Therefore, in
//# determining the length of the packet number encoding for 0-RTT
//# packets, a client MUST assume that all packets up to the current
//# packet number are in flight, starting from a packet number of 0.
//# Thus, 0-RTT packets could need to use a longer packet number
//# encoding.
//#
//# A client MUST NOT send 0-RTT packets once it starts processing 1-RTT
//# packets from the server.  This means that 0-RTT packets cannot
//# contain any response to frames from 1-RTT packets.  For instance, a
//# client cannot send an ACK frame in a 0-RTT packet, because that can
//# only acknowledge a 1-RTT packet.  An acknowledgment for a 1-RTT
//# packet MUST be carried in a 1-RTT packet.
//#
//# A server SHOULD treat a violation of remembered limits as a
//# connection error of an appropriate type (for instance, a
//# FLOW_CONTROL_ERROR for exceeding stream data limits).

#[derive(Debug)]
pub struct ZeroRTT<DCID, SCID, PacketNumber, Payload> {
    pub version: Version,
    pub destination_connection_id: DCID,
    pub source_connection_id: SCID,
    pub packet_number: PacketNumber,
    pub payload: Payload,
}

pub type ProtectedZeroRTT<'a> =
    ZeroRTT<CheckedRange, CheckedRange, ProtectedPacketNumber, ProtectedPayload<'a>>;
pub type EncryptedZeroRTT<'a> =
    ZeroRTT<CheckedRange, CheckedRange, PacketNumber, EncryptedPayload<'a>>;
pub type CleartextZeroRTT<'a> = ZeroRTT<&'a [u8], &'a [u8], PacketNumber, DecoderBufferMut<'a>>;

impl<'a> ProtectedZeroRTT<'a> {
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<ProtectedZeroRTT> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;

        let (payload, packet_number, remaining) =
            decoder.finish_long()?.split_off_packet(buffer)?;

        let packet = ZeroRTT {
            version,
            destination_connection_id,
            source_connection_id,
            packet_number,
            payload,
        };

        Ok((packet, remaining))
    }

    pub fn unprotect<C: ZeroRTTCrypto>(
        self,
        crypto: &C,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedZeroRTT<'a>, CryptoError> {
        let ZeroRTT {
            version,
            destination_connection_id,
            source_connection_id,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(crypto, PacketNumberSpace::ApplicationData, payload)?;

        let packet_number = truncated_packet_number
            .expand(largest_acknowledged_packet_number)
            .ok_or_else(CryptoError::decode_error)?;

        Ok(ZeroRTT {
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

impl<'a> EncryptedZeroRTT<'a> {
    pub fn decrypt<C: ZeroRTTCrypto>(
        self,
        crypto: &C,
    ) -> Result<CleartextZeroRTT<'a>, CryptoError> {
        let ZeroRTT {
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

        Ok(ZeroRTT {
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

impl<'a> CleartextZeroRTT<'a> {
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
    for ZeroRTT<DCID, SCID, TruncatedPacketNumber, Payload>
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
    ZeroRTT<DCID, SCID, PacketNumber, Payload>
{
    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        let mut tag: u8 = zero_rtt_tag!() << 4;
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
        Crypto: ZeroRTTCrypto + HeaderCrypto,
    > PacketEncoder<Crypto, Payload> for ZeroRTT<DCID, SCID, PacketNumber, Payload>
{
    type PayloadLenCursor = LongPayloadLenCursor;

    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    fn encode_header<E: Encoder>(&self, packet_number_len: PacketNumberLen, encoder: &mut E) {
        ZeroRTT::encode_header(self, packet_number_len, encoder);
    }

    fn payload(&mut self) -> &mut Payload {
        &mut self.payload
    }
}
