use crate::packet::{
    long::{
        validate_destination_connection_id_len, validate_source_connection_id_len,
        DestinationConnectionIDLen, SourceConnectionIDLen, Version,
    },
    Tag,
};
use core::mem::size_of;
use s2n_codec::{
    decoder_invariant, DecoderBuffer, DecoderBufferMut, DecoderBufferMutResult, Encoder,
    EncoderValue,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.1
//# Version Negotiation Packet {
//#   Header Form (1) = 1,
//#   Unused (7),
//#   Version (32) = 0,
//#   Destination Connection ID Length (8),
//#   Destination Connection ID (0..2040),
//#   Source Connection ID Length (8),
//#   Source Connection ID (0..2040),
//#   Supported Version (32) ...,
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.1
//# The value in the Unused field is selected randomly by the server.
//# Clients MUST ignore the value of this field.  Servers SHOULD set the
//# most significant bit of this field (0x40) to 1 so that Version
//# Negotiation packets appear to have the Fixed Bit field.

macro_rules! version_negotiation_no_fixed_bit_tag {
    () => {
        0b1000u8..=0b1011u8
    };
}

const ENCODING_TAG: u8 = 0b1100_0000;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.1
//# The Version field of a Version Negotiation packet MUST be set to
//# 0x00000000.

pub(crate) const VERSION: u32 = 0x0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionNegotiation<'a, SupportedVersions> {
    pub tag: Tag,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub supported_versions: SupportedVersions,
}

pub type ProtectedVersionNegotiation<'a> = VersionNegotiation<'a, &'a [u8]>;
pub type EncryptedVersionNegotiation<'a> = VersionNegotiation<'a, &'a [u8]>;
pub type CleartextVersionNegotiation<'a> = VersionNegotiation<'a, &'a [u8]>;

impl<'a> ProtectedVersionNegotiation<'a> {
    #[inline]
    pub fn decode(
        tag: Tag,
        _version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<VersionNegotiation<&[u8]>> {
        let buffer = buffer
            .skip(size_of::<Tag>() + size_of::<Version>())
            .expect("tag and version already verified");

        let (destination_connection_id, buffer) =
            buffer.decode_slice_with_len_prefix::<DestinationConnectionIDLen>()?;
        let destination_connection_id = destination_connection_id.into_less_safe_slice();
        validate_destination_connection_id_len(destination_connection_id.len())?;

        let (source_connection_id, buffer) =
            buffer.decode_slice_with_len_prefix::<SourceConnectionIDLen>()?;
        let source_connection_id = source_connection_id.into_less_safe_slice();
        validate_source_connection_id_len(source_connection_id.len())?;

        let (supported_versions, buffer) = buffer.decode::<DecoderBufferMut>()?;
        let supported_versions: &[u8] = supported_versions.into_less_safe_slice();

        // ensure at least one version is present
        decoder_invariant!(
            supported_versions.len() >= size_of::<u32>(),
            "missing at least one version"
        );

        // ensure payload length can successfully decode a list of u32s
        decoder_invariant!(
            supported_versions.len() % size_of::<u32>() == 0,
            "invalid payload length"
        );

        let packet = VersionNegotiation {
            tag,
            destination_connection_id,
            source_connection_id,
            supported_versions,
        };

        Ok((packet, buffer))
    }

    #[inline]
    pub fn iter(&'a self) -> VersionNegotiationIterator<'a> {
        self.into_iter()
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        self.destination_connection_id
    }
}

impl<'a> IntoIterator for ProtectedVersionNegotiation<'a> {
    type IntoIter = VersionNegotiationIterator<'a>;
    type Item = u32;

    fn into_iter(self) -> Self::IntoIter {
        VersionNegotiationIterator(DecoderBuffer::new(self.supported_versions))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VersionNegotiationIterator<'a>(DecoderBuffer<'a>);

impl<'a> Iterator for VersionNegotiationIterator<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Ok((value, buffer)) = self.0.decode() {
            self.0 = buffer;
            Some(value)
        } else {
            None
        }
    }
}

impl<'a, SupportedVersions: EncoderValue> EncoderValue
    for VersionNegotiation<'a, SupportedVersions>
{
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        (self.tag | ENCODING_TAG).encode(encoder);
        VERSION.encode(encoder);
        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, _>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, _>(encoder);
        self.supported_versions.encode(encoder);
    }
}
