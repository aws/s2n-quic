use crate::{
    crypto::{CryptoError, EncryptedPayload, HeaderCrypto, InitialCrypto, ProtectedPayload},
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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.2
//# 17.2.2.  Initial Packet
//#
//#    An Initial packet uses long headers with a type value of 0x0.  It
//#    carries the first CRYPTO frames sent by the client and server to
//#    perform key exchange, and carries ACKs in either direction.
//#
//#    +-+-+-+-+-+-+-+-+
//#    |1|1| 0 |R R|P P|
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
//#    |                         Token Length (i)                    ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                            Token (*)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                           Length (i)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                    Packet Number (8/16/24/32)               ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                          Payload (*)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                          Figure 11: Initial Packet

macro_rules! initial_tag {
    () => {
        0b1100u8
    };
}

//#    The Initial packet contains a long header as well as the Length and
//#    Packet Number fields.  The first byte contains the Reserved and
//#    Packet Number Length bits.  Between the SCID and Length fields, there
//#    are two additional field specific to the Initial packet.
//#
//#    Token Length:  A variable-length integer specifying the length of the
//#       Token field, in bytes.  This value is zero if no token is present.
//#       Initial packets sent by the server MUST set the Token Length field
//#       to zero; clients that receive an Initial packet with a non-zero
//#       Token Length field MUST either discard the packet or generate a
//#       connection error of type PROTOCOL_VIOLATION.
//#
//#    Token:  The value of the token that was previously provided in a
//#       Retry packet or NEW_TOKEN frame.
//#
//#    Payload:  The payload of the packet.
//#
//#    In order to prevent tampering by version-unaware middleboxes, Initial
//#    packets are protected with connection- and version-specific keys
//#    (Initial keys) as described in [QUIC-TLS].  This protection does not
//#    provide confidentiality or integrity against on-path attackers, but
//#    provides some level of protection against off-path attackers.
//#
//#    The client and server use the Initial packet type for any packet that
//#    contains an initial cryptographic handshake message.  This includes
//#    all cases where a new packet containing the initial cryptographic
//#    message needs to be created, such as the packets sent after receiving
//#    a Retry packet (Section 17.2.5).
//#
//#    A server sends its first Initial packet in response to a client
//#    Initial.  A server may send multiple Initial packets.  The
//#    cryptographic key exchange could require multiple round trips or
//#    retransmissions of this data.
//#
//#    The payload of an Initial packet includes a CRYPTO frame (or frames)
//#    containing a cryptographic handshake message, ACK frames, or both.
//#    PADDING and CONNECTION_CLOSE frames are also permitted.  An endpoint
//#    that receives an Initial packet containing other frames can either
//#    discard the packet as spurious or treat it as a connection error.
//#
//#    The first packet sent by a client always includes a CRYPTO frame that
//#    contains the entirety of the first cryptographic handshake message.
//#    This packet, and the cryptographic handshake message, MUST fit in a
//#    single UDP datagram (see Section 7).  The first CRYPTO frame sent
//#    always begins at an offset of 0 (see Section 7).
//#
//#    Note that if the server sends a HelloRetryRequest, the client will
//#    send a second Initial packet.  This Initial packet will continue the
//#
//#    cryptographic handshake and will contain a CRYPTO frame with an
//#    offset matching the size of the CRYPTO frame sent in the first
//#    Initial packet.  Cryptographic handshake messages subsequent to the
//#    first do not need to fit within a single UDP datagram.

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

        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;
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

    pub fn unprotect<C: InitialCrypto>(
        self,
        crypto: &C,
        largest_acknowledged_packet_number: PacketNumber,
    ) -> Result<EncryptedInitial<'a>, CryptoError> {
        let Initial {
            version,
            destination_connection_id,
            source_connection_id,
            token,
            payload,
            ..
        } = self;

        let (truncated_packet_number, payload) =
            crate::crypto::unprotect(crypto, PacketNumberSpace::Initial, payload)?;

        let packet_number = truncated_packet_number
            .expand(largest_acknowledged_packet_number)
            .ok_or_else(CryptoError::decode_error)?;

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
    pub fn decrypt<C: InitialCrypto>(
        self,
        crypto: &C,
    ) -> Result<CleartextInitial<'a>, CryptoError> {
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
}

impl<'a> CleartextInitial<'a> {
    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        &self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        &self.source_connection_id
    }

    #[inline]
    pub fn token(&self) -> &[u8] {
        &self.token
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
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
        self.token.encode_with_len_prefix::<VarInt, E>(encoder);
    }
}

impl<
        DCID: EncoderValue,
        SCID: EncoderValue,
        Token: EncoderValue,
        Payload: PacketPayloadEncoder,
        Crypto: InitialCrypto + HeaderCrypto,
    > PacketEncoder<Crypto, Payload> for Initial<DCID, SCID, Token, PacketNumber, Payload>
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
