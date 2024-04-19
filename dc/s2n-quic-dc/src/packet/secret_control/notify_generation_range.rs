// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(NOTIFY_GENERATION_RANGE);
impl_packet!(NotifyGenerationRange);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct NotifyGenerationRange {
    pub credential_id: credentials::Id,
    pub min_generation_id: u32,
    pub max_generation_id: u32,
}

impl NotifyGenerationRange {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &mut C) -> encrypt::Result<usize>
    where
        C: encrypt::Key,
    {
        let min_generation_id = self.min_generation_id;
        let max_generation_id = self.max_generation_id;

        encoder.encode(&Tag::default());
        encoder.encode(&&self.credential_id[..]);
        encoder.encode(&VarInt::from(min_generation_id));
        let max_generation_id = max_generation_id.max(min_generation_id);
        encoder.encode(&VarInt::from(max_generation_id - min_generation_id));

        encoder::finish(
            encoder,
            Nonce::NotifyGenerationRange {
                min_generation_id,
                max_generation_id,
            },
            crypto,
        )
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::NotifyGenerationRange {
            min_generation_id: self.min_generation_id,
            max_generation_id: self.max_generation_id,
        }
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        use s2n_quic_core::ensure;

        ensure!(self.min_generation_id <= self.max_generation_id, None);

        Some(())
    }
}

impl<'a> DecoderValue<'a> for NotifyGenerationRange {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (credential_id, buffer) = buffer.decode()?;
        let (min_generation_id, buffer) = decoder::sized::<u32>(buffer)?;
        let (relative_max_generation_id, buffer) = decoder::sized(buffer)?;
        let max_generation_id = min_generation_id
            .checked_add(relative_max_generation_id)
            .ok_or(DecoderError::InvariantViolation("generation id overflow"))?;
        let value = Self {
            credential_id,
            min_generation_id,
            max_generation_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(NotifyGenerationRange);
}
