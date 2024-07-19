// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::mem::size_of;

impl_tag!(UNKNOWN_PATH_SECRET);

const STATELESS_RESET_LEN: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Packet<'a> {
    #[allow(dead_code)]
    header: &'a [u8],
    value: UnknownPathSecret,
    crypto_tag: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn new_for_test(
        id: crate::credentials::Id,
        stateless_reset: &[u8; STATELESS_RESET_LEN],
    ) -> Packet<'_> {
        Packet {
            header: &[],
            value: UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id: id,
            },
            crypto_tag: &stateless_reset[..],
        }
    }

    #[inline]
    pub fn decode(buffer: DecoderBufferMut<'a>) -> Rm<Packet> {
        let header_len = decoder::header_len::<UnknownPathSecret>(buffer.peek())?;
        let ((header, value, crypto_tag), buffer) =
            decoder::header(buffer, header_len, STATELESS_RESET_LEN)?;
        let packet = Self {
            header,
            value,
            crypto_tag,
        };
        Ok((packet, buffer))
    }

    #[inline]
    pub fn credential_id(&self) -> &crate::credentials::Id {
        &self.value.credential_id
    }

    #[inline]
    pub fn authenticate(
        &self,
        stateless_reset: &[u8; STATELESS_RESET_LEN],
    ) -> Option<&UnknownPathSecret> {
        aws_lc_rs::constant_time::verify_slices_are_equal(self.crypto_tag, stateless_reset).ok()?;
        Some(&self.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct UnknownPathSecret {
    pub wire_version: WireVersion,
    pub credential_id: credentials::Id,
}

impl UnknownPathSecret {
    pub const PACKET_SIZE: usize =
        size_of::<Tag>() + size_of::<u8>() + size_of::<credentials::Id>() + STATELESS_RESET_LEN;

    #[inline]
    pub fn encode(
        &self,
        mut encoder: EncoderBuffer,
        stateless_reset_tag: &[u8; STATELESS_RESET_LEN],
    ) -> usize {
        let before = encoder.len();
        encoder.encode(&Tag::default());
        encoder.encode(&&self.credential_id[..]);
        encoder.encode(&self.wire_version);
        encoder.encode(&&stateless_reset_tag[..]);
        let after = encoder.len();
        after - before
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::UnknownPathSecret
    }
}

impl<'a> DecoderValue<'a> for UnknownPathSecret {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (credential_id, buffer) = buffer.decode()?;
        let (wire_version, buffer) = buffer.decode()?;
        let value = Self {
            wire_version,
            credential_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_test() {
        bolero::check!()
            .with_type::<(UnknownPathSecret, [u8; 16])>()
            .for_each(|(value, stateless_reset)| {
                let mut buffer = [0u8; UnknownPathSecret::PACKET_SIZE];
                let len = {
                    let encoder = s2n_codec::EncoderBuffer::new(&mut buffer);
                    value.encode(encoder, stateless_reset)
                };

                {
                    let buffer = s2n_codec::DecoderBufferMut::new(&mut buffer[..len]);
                    let (decoded, _) = Packet::decode(buffer).unwrap();
                    let decoded = decoded.authenticate(stateless_reset).unwrap();
                    assert_eq!(value, decoded);
                }
            });
    }
}
