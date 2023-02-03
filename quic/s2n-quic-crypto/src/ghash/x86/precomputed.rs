// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    arch::*,
    block::{x86::M128iExt, Batch, Block, Zeroed},
    ghash::{
        self,
        x86::{algo, hkey::HKey},
        KEY_LEN,
    },
};
use s2n_quic_core::assume;
use zeroize::{DefaultIsZeroes, Zeroize};

impl<P: Powers> ghash::GHash for P {
    type Block = __m128i;
    type State = State;

    #[inline(always)]
    fn start(&self, required_blocks: usize) -> Self::State {
        debug_assert!(self.capacity() >= required_blocks);
        State::new(required_blocks)
    }

    #[inline(always)]
    fn update<B: Batch<Block = Self::Block>>(&self, state: &mut Self::State, block: &B) {
        block.for_each(
            #[inline(always)]
            |_idx, b| {
                *state = state.update(self, b);
            },
        );
    }

    #[inline(always)]
    fn finish(&self, state: Self::State) -> Self::Block {
        state.finish()
    }
}

pub trait Powers {
    type HKey: HKey;

    fn power(&self, index: usize) -> &Self::HKey;
    fn capacity(&self) -> usize;
}

pub struct Allocated<H: HKey> {
    state: Box<[H]>,
}

impl<H: HKey> Allocated<H> {
    #[inline(always)]
    pub fn new(key: [u8; KEY_LEN], blocks: usize) -> Self {
        // initialize the powers (H^1, H^2, H^3, etc)
        let mut state = Vec::with_capacity(blocks);
        let mut current = H::new(__m128i::from_array(key));
        let first = current;
        state.push(first);

        // precompute the H value for each block
        for _ in 0..blocks {
            current = current.derive(&first);
            state.push(current);
        }

        let state = state.into_boxed_slice();

        Self { state }
    }
}

impl<H: HKey> Powers for Allocated<H> {
    type HKey = H;

    #[inline(always)]
    fn power(&self, index: usize) -> &H {
        unsafe {
            assume!(index < self.state.len());
            self.state.get_unchecked(index)
        }
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.state.len()
    }
}

impl<H: HKey + DefaultIsZeroes> Zeroize for Allocated<H> {
    #[inline]
    fn zeroize(&mut self) {
        // deref to a slice to we can take advantage of the bulk zeroization
        self.state.zeroize()
    }
}

pub struct Array<H: HKey, const N: usize> {
    state: [H; N],
}

impl<H: HKey, const N: usize> Array<H, N> {
    #[allow(dead_code)] // This is currently used in testing only
    #[inline(always)]
    pub fn new(key: [u8; KEY_LEN]) -> Self {
        // initialize the powers (H^1, H^2, H^3, etc)
        let mut state = [H::zeroed(); N];
        let mut current = H::new(__m128i::from_array(key));
        let first = current;
        state[0] = first;

        // precompute the H value for each block
        for power in state.iter_mut().skip(1) {
            current = current.derive(&first);
            *power = current;
        }

        Self { state }
    }
}

impl<H: HKey, const N: usize> Powers for Array<H, N> {
    type HKey = H;

    #[inline(always)]
    fn power(&self, index: usize) -> &H {
        unsafe {
            assume!(index < self.state.len());
            self.state.get_unchecked(index)
        }
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.state.len()
    }
}

impl<H: HKey + DefaultIsZeroes, const N: usize> Zeroize for Array<H, N> {
    #[inline]
    fn zeroize(&mut self) {
        // deref to a slice to we can take advantage of the bulk zeroization
        self.state.zeroize()
    }
}

#[derive(Clone, Copy, Zeroize)]
pub struct State {
    hi: __m128i,
    mid: __m128i,
    lo: __m128i,
    power: usize,
}

impl State {
    #[inline(always)]
    fn new(power: usize) -> Self {
        Self {
            hi: __m128i::zeroed(),
            mid: __m128i::zeroed(),
            lo: __m128i::zeroed(),
            power,
        }
    }

    #[inline(always)]
    // This implementation is written to closely follow the original code
    #[allow(unknown_lints, clippy::needless_late_init)]
    fn update<P: Powers>(&self, powers: &P, b: &__m128i) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            assume!(
                self.power != 0,
                "update called more than requested capacity"
            );

            let power = self.power - 1;
            let hkey = powers.power(power);

            let b = b.reverse();

            let mut t;
            let h = hkey.h();

            t = _mm_clmulepi64_si128(h, b, 0x00);
            let lo = self.lo.xor(t);

            t = _mm_clmulepi64_si128(h, b, 0x11);
            let hi = self.hi.xor(t);

            t = _mm_shuffle_epi32(b, 78);
            t = t.xor(b);
            t = _mm_clmulepi64_si128(hkey.r(), t, 0x00);
            let mid = self.mid.xor(t);

            Self { hi, mid, lo, power }
        }
    }

    #[inline(always)]
    fn finish(self) -> __m128i {
        let State {
            mut hi,
            mut mid,
            mut lo,
            power,
        } = self;

        unsafe {
            debug_assert!(Avx2::is_supported());
            assume!(
                power == 0,
                "ghash update count incorrect: remaining {}",
                power
            );

            mid = mid.xor(hi);
            mid = mid.xor(lo);
            lo = lo.xor(_mm_slli_si128(mid, 8));
            hi = hi.xor(_mm_srli_si128(mid, 8));

            let tag = algo::reduce(lo, hi);
            tag.reverse()
        }
    }
}
