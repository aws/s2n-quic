// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use s2n_codec::{
    decoder_value,
    zerocopy::{FromBytes, IntoBytes, Unaligned},
    zerocopy_value_codec, Encoder, EncoderValue,
};
pub use s2n_quic_core::varint::VarInt as KeyId;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[derive(Clone, Copy, Default, PartialEq, Eq, FromBytes, IntoBytes, Unaligned, PartialOrd, Ord)]
#[cfg_attr(
    any(test, feature = "testing"),
    derive(bolero_generator::TypeGenerator)
)]
#[repr(C)]
pub struct Id([u8; 16]);

impl Id {
    pub(crate) fn to_hash(self) -> u64 {
        // The ID has very high quality entropy already, so write just one half of it to keep hash
        // costs as low as possible. For the main use of the Hash impl in the fixed-size ID map
        // this translates to just directly using these bytes for the indexing.
        u64::from_ne_bytes(self.0[..8].try_into().unwrap())
    }
}

impl std::hash::Hash for Id {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.to_hash());
    }
}

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
#[cfg_attr(
    any(test, feature = "testing"),
    derive(bolero_generator::TypeGenerator)
)]
pub struct Credentials {
    pub id: Id,
    pub key_id: KeyId,
}

decoder_value!(
    impl<'a> Credentials {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (id, buffer) = buffer.decode()?;
            let (key_id, buffer) = buffer.decode::<KeyId>()?;
            Ok((Self { id, key_id }, buffer))
        }
    }
);

impl EncoderValue for Credentials {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.id.encode(encoder);
        self.key_id.encode(encoder);
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
