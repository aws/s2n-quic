// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::mem::size_of;

impl_tag!(UNKNOWN_PATH_SECRET);

#[derive(Clone, Copy, Debug)]
pub struct Packet<'a> {
    #[allow(dead_code)]
    header: &'a [u8],
    value: UnknownPathSecret,
    crypto_tag: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn new_for_test(id: crate::credentials::Id, stateless_reset: &[u8; TAG_LEN]) -> Packet<'_> {
        Packet {
            header: &[],
            value: UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id: id,
                queue_id: None,
            },
            crypto_tag: &stateless_reset[..],
        }
    }

    #[inline]
    pub fn decode(buffer: DecoderBufferMut<'a>) -> Rm<'a, Self> {
        let header_len = decoder::header_len::<UnknownPathSecret>(buffer.peek())?;
        let ((header, value, crypto_tag), buffer) = decoder::header(buffer, header_len)?;
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
    pub fn authenticate(&self, stateless_reset: &[u8; TAG_LEN]) -> Option<&UnknownPathSecret> {
        aws_lc_rs::constant_time::verify_slices_are_equal(self.crypto_tag, stateless_reset).ok()?;
        Some(&self.value)
    }

    #[inline]
    pub fn queue_id(&self) -> Option<VarInt> {
        self.value.queue_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct UnknownPathSecret {
    pub credential_id: credentials::Id,
    pub wire_version: WireVersion,
    pub queue_id: Option<VarInt>,
}

impl UnknownPathSecret {
    pub const MAX_PACKET_SIZE: usize = size_of::<Tag>()
        + size_of::<u8>()
        + size_of::<credentials::Id>()
        + size_of::<VarInt>()
        + TAG_LEN;

    #[inline]
    pub fn encode(&self, mut encoder: EncoderBuffer, stateless_reset_tag: &[u8; TAG_LEN]) -> usize {
        let before = encoder.len();
        encoder.encode(&Tag::default().with_queue_id(self.queue_id.is_some()));
        encoder.encode(&&self.credential_id[..]);
        encoder.encode(&self.wire_version);
        if let Some(queue_id) = self.queue_id {
            encoder.encode(&queue_id);
        }
        encoder.encode(&&stateless_reset_tag[..]);
        let after = encoder.len();
        after - before
    }
}

impl<'a> DecoderValue<'a> for UnknownPathSecret {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        let (credential_id, buffer) = buffer.decode()?;
        let (wire_version, buffer) = buffer.decode()?;
        let (queue_id, buffer) = if tag.has_queue_id() {
            let (queue_id, buffer) = buffer.decode()?;
            (Some(queue_id), buffer)
        } else {
            (None, buffer)
        };

        let value = Self {
            wire_version,
            credential_id,
            queue_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateless_reset_len() {
        assert_eq!(s2n_quic_core::stateless_reset::token::LEN, TAG_LEN);
    }

    #[test]
    fn round_trip_test() {
        bolero::check!()
            .with_type::<(UnknownPathSecret, [u8; TAG_LEN])>()
            .for_each(|(value, stateless_reset)| {
                let mut buffer = [0u8; UnknownPathSecret::MAX_PACKET_SIZE];
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
