// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl_tag!(REQUEST_ADDITIONAL_GENERATION);
impl_packet!(RequestAdditionalGeneration);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct RequestAdditionalGeneration {
    pub credential_id: credentials::Id,
    pub generation_id: u32,
}

impl RequestAdditionalGeneration {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &mut C) -> encrypt::Result<usize>
    where
        C: encrypt::Key,
    {
        let generation_id = self.generation_id;

        encoder.encode(&Tag::default());
        encoder.encode(&&self.credential_id[..]);
        encoder.encode(&VarInt::from(generation_id));

        encoder::finish(
            encoder,
            Nonce::RequestAdditionalGeneration { generation_id },
            crypto,
        )
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::RequestAdditionalGeneration {
            generation_id: self.generation_id,
        }
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        Some(())
    }
}

impl<'a> DecoderValue<'a> for RequestAdditionalGeneration {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (credential_id, buffer) = buffer.decode()?;
        let (generation_id, buffer) = decoder::sized(buffer)?;
        let value = Self {
            credential_id,
            generation_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(RequestAdditionalGeneration);
}
