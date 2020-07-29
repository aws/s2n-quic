use crate::packet::{
    decoding::HeaderDecoder,
    long::{DestinationConnectionIDLen, SourceConnectionIDLen, Version},
    Tag,
};
use crate::crypto::pseudo_retry::{NONCE, SECRET_KEY};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderBuffer, EncoderValue};
pub use ring::aead;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29#17.2.5
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
//#  }
//#
//#                          Figure 17: Retry Packet

macro_rules! retry_tag {
    () => {
        0b1111u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29#17.2.5
//#
//#   A Retry packet (shown in Figure 17) does not contain any protected
//#   fields.  The value in the Unused field is selected randomly by the
//#   server.  In addition to the fields from the long header, it contains
//#   these additional fields:
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

//= https://tools.ietf.org/html/draft-ietf-quic-tls-29#section-5.8
//# Retry Pseudo-Packet {
//#     ODCID Length (8),
//#     Original Destination Connection ID (0..160),
//#     Header Form (1) = 1,
//#     Fixed Bit (1) = 1,
//#     Long Packet Type (2) = 3,
//#     Type-Specific Bits (4),
//#     Version (32),
//#     DCID Len (8),
//#     Destination Connection ID (0..160),
//#     SCID Len (8),
//#     Retry Token (..),
//#   }
//#
//#                       Figure 8: Retry Pseudo-Packet
//#
//#   The Retry Pseudo-Packet is not sent over the wire.  It is computed by
//#   taking the transmitted Retry packet, removing the Retry Integrity Tag
//#   and prepending the two following fields:
//#
//#   ODCID Length:  The ODCID Len contains the length in bytes of the
//#      Original Destination Connection ID field that follows it, encoded
//#      as an 8-bit unsigned integer.
//#
//#   Original Destination Connection ID:  The Original Destination
//#      Connection ID contains the value of the Destination Connection ID
//#      from the Initial packet that this Retry is in response to.  The
//#      length of this field is given in ODCID Len. The presence of this
//#      field mitigates an off-path attacker's ability to inject a Retry
//#      packet.

#[derive(Debug)]
pub struct PseudoRetry<'a> {
    pub original_destination_connection_id: &'a [u8],
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
}

impl<'a> From<Retry<'a>> for PseudoRetry<'a> {
    fn from(packet: Retry<'a>) -> Self {
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


impl PseudoRetry<'_> {
    pub fn calculate_integrity_tag(&self) -> [u8; 16] {

        let packet_len = self.original_destination_connection_id.len()
            + self.destination_connection_id.len()
            + self.source_connection_id.len();

        let mut pseudo_packet_buffer = Vec::with_capacity(packet_len);
        let mut pseudo_packet = EncoderBuffer::new(&mut pseudo_packet_buffer);

        pseudo_packet.encode(self);

        let nonce = aead::Nonce::assume_unique_for_key(NONCE);
        let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &SECRET_KEY).unwrap(),);
        let tag = key.seal_in_place_separate_tag(nonce, aead::Aad::from(pseudo_packet_buffer), &mut []).unwrap();

        let mut integrity_tag = [0; 16];
        integrity_tag.copy_from_slice(tag.as_ref());
        integrity_tag
    }
}
