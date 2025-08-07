// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(REPLAY_DETECTED);
impl_packet!(ReplayDetected);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct ReplayDetected {
    pub credential_id: credentials::Id,
    pub wire_version: WireVersion,
    pub queue_id: Option<VarInt>,
    pub rejected_key_id: VarInt,
}

impl ReplayDetected {
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
        encoder.encode(&self.rejected_key_id);

        encoder::finish(encoder, crypto)
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        Some(())
    }
}

impl<'a> DecoderValue<'a> for ReplayDetected {
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
        let (rejected_key_id, buffer) = buffer.decode()?;
        let value = Self {
            credential_id,
            wire_version,
            queue_id,
            rejected_key_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(ReplayDetected);
}
