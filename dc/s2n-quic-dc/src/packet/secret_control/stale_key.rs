// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(STALE_KEY);
impl_packet!(StaleKey);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct StaleKey {
    pub wire_version: WireVersion,
    pub credential_id: credentials::Id,
    pub min_key_id: VarInt,
}

impl StaleKey {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &C) -> usize
    where
        C: encrypt::Key,
    {
        encoder.encode(&Tag::default());
        encoder.encode(&self.wire_version);
        encoder.encode(&self.credential_id);
        encoder.encode(&self.min_key_id);

        encoder::finish(encoder, self.nonce(), crypto)
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::StaleKey {
            min_key_id: self.min_key_id.into(),
        }
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        Some(())
    }
}

impl<'a> DecoderValue<'a> for StaleKey {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (wire_version, buffer) = buffer.decode()?;
        let (credential_id, buffer) = buffer.decode()?;
        let (min_key_id, buffer) = buffer.decode()?;
        let value = Self {
            wire_version,
            credential_id,
            min_key_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(StaleKey);
}
