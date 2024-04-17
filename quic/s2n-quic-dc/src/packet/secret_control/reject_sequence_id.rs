use super::*;

impl_tag!(REJECT_SEQUENCE_ID);
impl_packet!(RejectSequenceId);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct RejectSequenceId {
    pub credential_id: credentials::Id,
    pub generation_id: u32,
    pub sequence_id: u16,
    pub max_generation_id: u32,
}

impl RejectSequenceId {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &mut C) -> encrypt::Result<usize>
    where
        C: encrypt::Key,
    {
        let generation_id = self.generation_id;
        let sequence_id = self.sequence_id;
        let max_generation_id = self.max_generation_id;

        encoder.encode(&Tag::default());
        encoder.encode(&credentials::Credentials {
            id: self.credential_id,
            generation_id,
            sequence_id,
        });
        // make sure the generation_id is included in the max seen
        let max_generation_id = generation_id.max(max_generation_id);
        encoder.encode(&VarInt::from(max_generation_id - generation_id));

        encoder::finish(
            encoder,
            Nonce::RejectSequenceId {
                generation_id,
                sequence_id,
                max_generation_id,
            },
            crypto,
        )
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        Nonce::RejectSequenceId {
            sequence_id: self.sequence_id,
            generation_id: self.generation_id,
            max_generation_id: self.max_generation_id,
        }
    }

    #[cfg(test)]
    fn validate(&self) -> Option<()> {
        use s2n_quic_core::ensure;

        ensure!(self.generation_id <= self.max_generation_id, None);

        Some(())
    }
}

impl<'a> DecoderValue<'a> for RejectSequenceId {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (tag, buffer) = buffer.decode::<Tag>()?;
        decoder_invariant!(tag == Tag::default(), "invalid tag");
        let (credentials, buffer) = buffer.decode::<credentials::Credentials>()?;
        let credential_id = credentials.id;
        let generation_id = credentials.generation_id;
        let sequence_id = credentials.sequence_id;
        let (relative_max_generation_id, buffer) = decoder::sized(buffer)?;
        let max_generation_id = generation_id
            .checked_add(relative_max_generation_id)
            .ok_or(DecoderError::InvariantViolation("generation id overflow"))?;
        let value = Self {
            credential_id,
            generation_id,
            sequence_id,
            max_generation_id,
        };
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl_tests!(RejectSequenceId);
}
