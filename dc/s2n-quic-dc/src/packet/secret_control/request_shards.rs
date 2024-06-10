// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(REQUEST_SHARDS);
impl_packet!(RequestShards);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct RequestShards {
    pub credential_id: credentials::Id,
    pub receiving_shards: u16,
    pub shard_width: u64,
}

impl RequestShards {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &mut C) -> usize
    where
        C: encrypt::Key,
    {
        encoder.encode(&Tag::default());
        encoder.encode(&self.credential_id);
        encoder.encode(&VarInt::from(self.receiving_shards));
        encoder.encode(&self.shard_width);

        encoder::finish(encoder, self.nonce(), crypto)
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::RequestShards {
            receiving_shards: self.receiving_shards,
            shard_width: self.shard_width,
        }
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        Some(())
    }
}

impl<'a> DecoderValue<'a> for RequestShards {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (credential_id, buffer) = buffer.decode()?;
        let (receiving_shards, buffer) = buffer.decode::<VarInt>()?;
        let (shard_width, buffer) = buffer.decode()?;
        let value = Self {
            credential_id,
            receiving_shards: receiving_shards
                .try_into()
                .map_err(|_| DecoderError::InvariantViolation("receiving_shards too big"))?,
            shard_width,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(RequestShards);
}
