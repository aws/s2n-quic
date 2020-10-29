use crate::packet::{
    decoding::HeaderDecoder,
    long::{DestinationConnectionIDLen, SourceConnectionIDLen, Version},
    Tag,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-31.txt#17.2.5
//#   A Retry packet uses a long packet header with a type value of 0x3.
//#   It carries an address validation token created by the server.  It is
//#   used by a server that wishes to perform a retry; see Section 8.1.
//#
//#   Retry Packet {
//#     Header Form (1) = 1,
//#     Fixed Bit (1) = 1,
//#     Long Packet Type (2) = 3,
//#     Unused (4),
//#     Version (32),
//#     Destination Connection ID Length (8),
//#     Destination Connection ID (0..160),
//#     Source Connection ID Length (8),
//#     Source Connection ID (0..160),
//#     Retry Token (..),
//#     Retry Integrity Tag (128),
//#   }
//#
//#                          Figure 18: Retry Packet

macro_rules! retry_tag {
    () => {
        0b1111u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-31.txt#17.2.5
//#   A Retry packet (shown in Figure 18) does not contain any protected
//#   fields.  The value in the Unused field is set to an arbitrary value
//#   by the server; a client MUST ignore these bits.  In addition to the
//#   fields from the long header, it contains these additional fields:
//#
//#   Retry Token:  An opaque token that the server can use to validate the
//#      client's address.
//#
//#   Retry Integrity Tag:  See the Retry Packet Integrity section of
//#      [QUIC-TLS].

#[derive(Debug)]
pub struct Retry<'a> {
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
    pub retry_integrity_tag: &'a [u8],
}

pub type ProtectedRetry<'a> = Retry<'a>;
pub type EncryptedRetry<'a> = Retry<'a>;
pub type CleartextRetry<'a> = Retry<'a>;

impl<'a> Retry<'a> {
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<Retry> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;

        // split header and payload
        let header_len = decoder.decoded_len();
        let (header, buffer) = buffer.decode_slice(header_len)?;
        let header: &[u8] = header.into_less_safe_slice();

        // read borrowed slices
        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);

        let remaining_bytes = buffer.len();
        let (retry_token, buffer) = buffer.decode_slice(remaining_bytes)?;
        let retry_token: &[u8] = retry_token.into_less_safe_slice();

        let remaining_bytes = buffer.len();
        let (retry_integrity_tag, buffer) = buffer.decode_slice(remaining_bytes)?;
        let retry_integrity_tag: &[u8] = retry_integrity_tag.into_less_safe_slice();

        let packet = Retry {
            version,
            destination_connection_id,
            source_connection_id,
            retry_token,
            retry_integrity_tag,
        };

        Ok((packet, buffer))
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        &self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        &self.source_connection_id
    }

    #[inline]
    pub fn retry_token(&self) -> &[u8] {
        &self.retry_token
    }

    #[inline]
    pub fn retry_integrity_tag(&self) -> &[u8] {
        &self.retry_integrity_tag
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        let pseudo_packet: PseudoRetry = self.into();

        pseudo_packet.integrity_tag() == self.retry_integrity_tag()
    }
}

impl<'a> EncoderValue for Retry<'a> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        let tag: u8 = retry_tag!() << 4;
        tag.encode(encoder);

        self.version.encode(encoder);

        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
        self.retry_token.encode(encoder);
        self.retry_integrity_tag.encode(encoder);
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-31.txt#5.8
//# Retry Pseudo-Packet {
//#   ODCID Length (8),
//#   Original Destination Connection ID (0..160),
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2) = 3,
//#   Type-Specific Bits (4),
//#   Version (32),
//#   DCID Len (8),
//#   Destination Connection ID (0..160),
//#   SCID Len (8),
//#   Source Connection ID (0..160),
//#   Retry Token (..),
//# }
#[derive(Debug)]
pub struct PseudoRetry<'a> {
    pub original_destination_connection_id: &'a [u8],
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
}

impl<'a> PseudoRetry<'a> {
    pub fn integrity_tag(&self) -> &[u8] {
        // RetryCrypto::calculate_tag(&self)
        todo!()
    }
}

impl<'a> From<&Retry<'a>> for PseudoRetry<'a> {
    fn from(packet: &Retry<'a>) -> Self {
        Self {
            original_destination_connection_id: packet.destination_connection_id,
            version: packet.version,
            destination_connection_id: packet.destination_connection_id,
            source_connection_id: packet.source_connection_id,
            retry_token: packet.retry_token,
        }
    }
}

impl<'a> EncoderValue for PseudoRetry<'a> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.original_destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);

        let tag: u8 = retry_tag!() << 4;
        tag.encode(encoder);

        self.version.encode(encoder);

        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
        self.retry_token.encode(encoder);
    }
}
