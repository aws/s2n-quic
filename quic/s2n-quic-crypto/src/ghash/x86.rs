// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    arch::*,
    block::{x86::M128iExt, Batch, Block, Zeroed},
    ghash::KEY_LEN,
};
use zeroize::Zeroize;

mod algo;
pub mod hkey;
pub mod precomputed;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[derive(Zeroize)]
pub struct GHash(hkey::H);

impl GHash {
    #[allow(dead_code)] // this is currently used in testing only
    #[inline(always)]
    pub fn new(key: [u8; KEY_LEN]) -> Self {
        use hkey::HKey;
        Self(hkey::H::new(__m128i::from_array(key)))
    }
}

impl super::GHash for GHash {
    type Block = __m128i;
    type State = __m128i;

    #[inline(always)]
    fn start(&self, _required_blocks: usize) -> Self::State {
        __m128i::zeroed()
    }

    #[inline(always)]
    fn update<B: Batch<Block = Self::Block>>(&self, state: &mut Self::State, block: &B) {
        let mut y = *state;
        block.for_each(
            #[inline(always)]
            |_idx, b| {
                y = self.0.mul(b.reverse().xor(y));
            },
        );
        *state = y;
    }

    #[inline(always)]
    fn finish(&self, state: Self::State) -> Self::Block {
        let y = state;

        y.reverse()
    }
}
