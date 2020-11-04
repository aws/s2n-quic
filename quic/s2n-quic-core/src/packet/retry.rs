use crate::packet::{
    decoding::HeaderDecoder,
    long::{DestinationConnectionIDLen, SourceConnectionIDLen, Version},
    Tag,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5
//# Retry Packet {
//#   Header Form (1) = 1,
//#   Fixed Bit (1) = 1,
//#   Long Packet Type (2) = 3,
//#   Unused (4),
//#   Version (32),
//#   Destination Connection ID Length (8),
//#   Destination Connection ID (0..160),
//#   Source Connection ID Length (8),
//#   Source Connection ID (0..160),
//#   Retry Token (..),
//#   Retry Integrity Tag (128),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5
//# A Retry packet uses a long packet header with a type value of 0x3.
macro_rules! retry_tag {
    () => {
        0b1111u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5
//#   Retry Token:  An opaque token that the server can use to validate the
//#      client's address.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5
//#   Retry Integrity Tag:  See the Retry Packet Integrity section of
//#      [QUIC-TLS].

#[derive(Debug)]
pub struct Retry<'a> {
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub original_destination_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
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
        let original_destination_connection_id =
            decoder.decode_destination_connection_id(&buffer)?;

        // split header and payload
        let header_len = decoder.decoded_len();
        let (header, buffer) = buffer.decode_slice(header_len)?;
        let header: &[u8] = header.into_less_safe_slice();

        // read borrowed slices
        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);
        let original_destination_connection_id = original_destination_connection_id.get(header);

        let buffer_len = buffer.len();
        let (retry_token, buffer) = buffer.decode_slice(buffer_len)?;
        let retry_token: &[u8] = retry_token.into_less_safe_slice();

        let packet = Retry {
            version,
            destination_connection_id,
            source_connection_id,
            original_destination_connection_id,
            retry_token,
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
    pub fn original_destination_connection_id(&self) -> &[u8] {
        &self.original_destination_connection_id
    }

    #[inline]
    pub fn retry_token(&self) -> &[u8] {
        &self.retry_token
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
        self.original_destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.retry_token.encode(encoder);
    }
}
