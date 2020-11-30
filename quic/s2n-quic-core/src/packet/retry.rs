use crate::{
    crypto::retry,
    packet::{
        decoding::HeaderDecoder,
        initial::ProtectedInitial,
        long::{DestinationConnectionIDLen, SourceConnectionIDLen, Version},
        Tag,
    },
};
use s2n_codec::{
    decoder_invariant, DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue,
};

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
    pub tag: Tag,
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
    pub retry_integrity_tag: Option<&'a [u8]>,
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.8
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
    pub tag: Tag,
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
}

impl<'a> PseudoRetry<'a> {
    pub fn new(
        odcid: &'a [u8],
        tag: Tag,
        version: Version,
        destination_connection_id: &'a [u8],
        source_connection_id: &'a [u8],
        retry_token: &'a [u8],
    ) -> Self {
        Self {
            original_destination_connection_id: odcid,
            tag,
            version,
            destination_connection_id,
            source_connection_id,
            retry_token,
        }
    }
}

pub type ProtectedRetry<'a> = Retry<'a>;
pub type EncryptedRetry<'a> = Retry<'a>;
pub type CleartextRetry<'a> = Retry<'a>;

impl<'a> Retry<'a> {
    pub fn from_initial(initial_packet: &'a ProtectedInitial) -> Self {
        Self {
            tag: retry_tag!(),
            version: initial_packet.version,
            destination_connection_id: initial_packet.destination_connection_id(),
            source_connection_id: initial_packet.source_connection_id(),
            retry_token: &[0u8; 512],
            retry_integrity_tag: None,
        }
    }

    #[inline]
    pub(crate) fn decode(
        tag: Tag,
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

        let buffer_len = buffer.len().saturating_sub(retry::INTEGRITY_TAG_LEN);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5.2
        //# A client MUST discard a Retry packet with a zero-length
        //# Retry Token field.
        decoder_invariant!(buffer_len > 0, "Token cannot be empty");

        let (retry_token, buffer) = buffer.decode_slice(buffer_len)?;
        let retry_token: &[u8] = retry_token.into_less_safe_slice();

        let (retry_integrity_tag, buffer) = buffer.decode_slice(retry::INTEGRITY_TAG_LEN)?;
        let retry_integrity_tag: &[u8] = retry_integrity_tag.into_less_safe_slice();

        let packet = Retry {
            tag,
            version,
            destination_connection_id,
            source_connection_id,
            retry_token,
            retry_integrity_tag: Some(retry_integrity_tag),
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
    pub fn pseudo_packet(&self, odcid: &'a [u8]) -> PseudoRetry {
        PseudoRetry {
            original_destination_connection_id: odcid,
            tag: self.tag,
            version: self.version,
            destination_connection_id: self.destination_connection_id,
            source_connection_id: self.source_connection_id,
            retry_token: self.retry_token,
        }
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
    }
}

impl<'a> EncoderValue for PseudoRetry<'a> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.original_destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);

        self.tag.encode(encoder);

        self.version.encode(encoder);

        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
        self.retry_token.encode(encoder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{crypto::retry, packet};
    use s2n_codec::EncoderBuffer;

    #[test]
    fn test_decode() {
        let mut buf = retry::example::PACKET;
        let decoder = DecoderBufferMut::new(&mut buf);
        let remote_address = crate::inet::ip::SocketAddress::default();
        let connection_info = crate::connection::id::ConnectionInfo::new(&remote_address);
        let (packet, _) = packet::ProtectedPacket::decode(decoder, &connection_info, &20).unwrap();
        let packet = match packet {
            packet::ProtectedPacket::Retry(retry) => retry,
            _ => panic!("expected retry packet type"),
        };

        assert_eq!(
            packet.retry_integrity_tag.unwrap(),
            retry::example::EXPECTED_TAG
        );
        assert_eq!(packet.retry_token, retry::example::TOKEN);
        assert_eq!(packet.source_connection_id, retry::example::SCID);
        assert_eq!(packet.destination_connection_id, retry::example::DCID);
        assert_eq!(packet.version, retry::example::VERSION);
    }

    #[test]
    fn test_pseudo_decode() {
        let mut buf = retry::example::PACKET;
        let decoder = DecoderBufferMut::new(&mut buf);
        let remote_address = crate::inet::ip::SocketAddress::default();
        let connection_info = crate::connection::id::ConnectionInfo::new(&remote_address);
        let (packet, _) = packet::ProtectedPacket::decode(decoder, &connection_info, &20).unwrap();
        let packet = match packet {
            packet::ProtectedPacket::Retry(retry) => retry,
            _ => panic!("expected retry packet type"),
        };
        let pseudo_packet = packet.pseudo_packet(&retry::example::ODCID);

        assert_eq!(pseudo_packet.retry_token, retry::example::TOKEN);
        assert_eq!(pseudo_packet.source_connection_id, retry::example::SCID);
        assert_eq!(
            pseudo_packet.destination_connection_id,
            retry::example::DCID
        );
        assert_eq!(pseudo_packet.version, retry::example::VERSION);
        assert_eq!(
            pseudo_packet.original_destination_connection_id,
            retry::example::ODCID
        );

        let length = pseudo_packet.encoding_size();
        let mut pseudo_scratch: Vec<u8> = vec![0; length];
        let mut encoder = EncoderBuffer::new(&mut pseudo_scratch);
        pseudo_packet.encode(&mut encoder);

        assert_eq!(pseudo_scratch, retry::example::PSEUDO_PACKET);
    }
}
