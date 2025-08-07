// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(STALE_KEY);
impl_packet!(StaleKey);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct StaleKey {
    pub credential_id: credentials::Id,
    pub wire_version: WireVersion,
    pub queue_id: Option<VarInt>,
    pub min_key_id: VarInt,
}

impl StaleKey {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &C) -> usize
    where
        C: seal::control::Secret,
    {
        encoder.encode(&Tag::default().with_queue_id(self.queue_id.is_some()));
        encoder.encode(&self.credential_id);
        encoder.encode(&self.wire_version);
        if let Some(queue_id) = self.queue_id {
            encoder.encode(&queue_id);
        }
        encoder.encode(&self.min_key_id);

        encoder::finish(encoder, crypto)
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
        let (credential_id, buffer) = buffer.decode()?;
        let (wire_version, buffer) = buffer.decode()?;
        let (queue_id, buffer) = if tag.has_queue_id() {
            let (queue_id, buffer) = buffer.decode()?;
            (Some(queue_id), buffer)
        } else {
            (None, buffer)
        };
        let (min_key_id, buffer) = buffer.decode()?;
        let value = Self {
            wire_version,
            credential_id,
            queue_id,
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
