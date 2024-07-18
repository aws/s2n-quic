// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(REPLAY_DETECTED);
impl_packet!(ReplayDetected);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct ReplayDetected {
    pub wire_version: WireVersion,
    pub credential_id: credentials::Id,
    pub rejected_key_id: VarInt,
}

impl ReplayDetected {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &C) -> usize
    where
        C: encrypt::Key,
    {
        encoder.encode(&Tag::default());
        encoder.encode(&self.wire_version);
        encoder.encode(&self.credential_id);
        encoder.encode(&self.rejected_key_id);

        encoder::finish(encoder, self.nonce(), crypto)
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::ReplayDetected {
            rejected_key_id: self.rejected_key_id.into(),
        }
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
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (wire_version, buffer) = buffer.decode()?;
        let (credential_id, buffer) = buffer.decode()?;
        let (rejected_key_id, buffer) = buffer.decode()?;
        let value = Self {
            wire_version,
            credential_id,
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
