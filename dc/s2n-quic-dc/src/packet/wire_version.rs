// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::{decoder_invariant, decoder_value, Encoder, EncoderValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct WireVersion(#[cfg_attr(test, generator(bolero_generator::constant(0)))] pub u32);

impl WireVersion {
    pub const ZERO: Self = Self(0);
}

decoder_value!(
    impl<'a> WireVersion {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (version, buffer) = buffer.decode::<u8>()?;
            decoder_invariant!(version == 0, "only wire version 0 is supported currently");
            let version = Self(version as _);
            Ok((version, buffer))
        }
    }
);

impl EncoderValue for WireVersion {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        debug_assert!(self.0 <= u8::MAX as u32);
        let v = self.0 as u8;
        v.encode(encoder);
    }
}
