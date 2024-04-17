use s2n_codec::{decoder_invariant, decoder_value};
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

#[derive(Clone, Copy, Debug, PartialEq, Eq, AsBytes, FromBytes, FromZeroes, Unaligned)]
#[repr(C)]
pub(super) struct Common(pub(super) u8);

impl Common {
    #[inline]
    pub(super) fn set(&mut self, mask: u8, enabled: bool) {
        self.0 = self.0 & !mask | if enabled { mask } else { 0 }
    }

    #[inline]
    pub(super) fn get(&self, mask: u8) -> bool {
        self.0 & mask != 0
    }

    #[inline]
    fn validate(&self) -> Result<(), s2n_codec::DecoderError> {
        decoder_invariant!(self.0 & 0b1000_0000 == 0, "only short packets are used");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Tag {
    Stream(super::stream::Tag),
    Datagram(super::datagram::Tag),
    Control(super::control::Tag),
    NotifyGenerationRange(super::secret_control::notify_generation_range::Tag),
    RejectSequenceId(super::secret_control::reject_sequence_id::Tag),
    RequestAdditionalGeneration(super::secret_control::request_additional_generation::Tag),
    UnknownPathSecret(super::secret_control::unknown_path_secret::Tag),
}

decoder_value!(
    impl<'a> Tag {
        fn decode(buffer: Buffer) -> Result<Self> {
            match buffer.peek_byte(0)? {
                super::stream::Tag::MIN..=super::stream::Tag::MAX => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::Stream(tag), buffer))
                }
                super::datagram::Tag::MIN..=super::datagram::Tag::MAX => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::Datagram(tag), buffer))
                }
                super::control::Tag::MIN..=super::control::Tag::MAX => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::Control(tag), buffer))
                }
                super::secret_control::notify_generation_range::Tag::VALUE => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::NotifyGenerationRange(tag), buffer))
                }
                super::secret_control::reject_sequence_id::Tag::VALUE => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::RejectSequenceId(tag), buffer))
                }
                super::secret_control::request_additional_generation::Tag::VALUE => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::RequestAdditionalGeneration(tag), buffer))
                }
                super::secret_control::unknown_path_secret::Tag::VALUE => {
                    let (tag, buffer) = buffer.decode()?;
                    Ok((Self::UnknownPathSecret(tag), buffer))
                }
                // reserve this range for other packet types
                0b0110_0000..=0b0111_1111 => Err(s2n_codec::DecoderError::InvariantViolation(
                    "unexpected packet tag",
                )),
                0b1000_0000..=0b1111_1111 => Err(s2n_codec::DecoderError::InvariantViolation(
                    "only short packets are accepted",
                )),
            }
        }
    }
);

macro_rules! impl_tag_codec {
    ($ty:ty) => {
        impl s2n_codec::EncoderValue for $ty {
            #[inline]
            fn encode<E: s2n_codec::Encoder>(&self, encoder: &mut E) {
                self.0.encode(encoder);
            }
        }

        s2n_codec::decoder_value!(
            impl<'a> $ty {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (byte, buffer) = buffer.decode()?;
                    let v = Self(byte);
                    v.validate()?;
                    Ok((v, buffer))
                }
            }
        );
    };
}

impl_tag_codec!(Common);
