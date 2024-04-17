use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use s2n_codec::{
    decoder_invariant, decoder_value,
    zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned},
    zerocopy_value_codec, Encoder, EncoderValue,
};
use s2n_quic_core::{assume, varint::VarInt};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsBytes,
    FromBytes,
    FromZeroes,
    Unaligned,
    PartialOrd,
    Ord,
)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
#[repr(C)]
pub struct Id([u8; 16]);

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        format_args!("{:#01x}", u128::from_be_bytes(self.0)).fmt(f)
    }
}

impl From<[u8; 16]> for Id {
    #[inline]
    fn from(v: [u8; 16]) -> Self {
        Self(v)
    }
}

impl Deref for Id {
    type Target = [u8; 16];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Id {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl s2n_quic_core::probe::Arg for Id {
    #[inline]
    fn into_usdt(self) -> isize {
        // we have to truncate the bytes, but 64 bits should be unique enough for these purposes
        let slice = &self.0[..core::mem::size_of::<usize>()];
        let bytes = slice.try_into().unwrap();
        usize::from_ne_bytes(bytes).into_usdt()
    }
}

zerocopy_value_codec!(Id);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct Credentials {
    pub id: Id,
    pub generation_id: u32,
    pub sequence_id: u16,
}

const MAX_VALUE: u64 = 1 << (32 + 16);

decoder_value!(
    impl<'a> Credentials {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (id, buffer) = buffer.decode()?;
            let (value, buffer) = buffer.decode::<VarInt>()?;
            let value = *value;
            decoder_invariant!(value <= MAX_VALUE, "invalid range");
            let generation_id = (value >> 16) as u32;
            let sequence_id = value as u16;
            Ok((
                Self {
                    id,
                    generation_id,
                    sequence_id,
                },
                buffer,
            ))
        }
    }
);

impl EncoderValue for Credentials {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.id.encode(encoder);
        let generation_id = (self.generation_id as u64) << 16;
        let sequence_id = self.sequence_id as u64;
        let value = generation_id | sequence_id;
        let value = unsafe {
            assume!(value <= MAX_VALUE);
            VarInt::new_unchecked(value)
        };
        value.encode(encoder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use s2n_codec::assert_codec_round_trip_value;

    #[test]
    fn round_trip_test() {
        check!().with_type::<Credentials>().for_each(|v| {
            assert_codec_round_trip_value!(Credentials, v);
        })
    }
}
