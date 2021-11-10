// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    arch::*,
    block::{x86::M128iExt, Block, Zeroed},
    ghash::x86::algo,
};

pub trait HKey: Copy + Zeroed {
    fn new(h: __m128i) -> Self;
    fn derive(&self, initial: &Self) -> Self;
    fn h(&self) -> __m128i;
    fn r(&self) -> __m128i;
}

#[derive(Clone, Copy)]
pub struct H(__m128i);

impl H {
    #[inline(always)]
    pub fn mul(self, y: __m128i) -> __m128i {
        unsafe {
            debug_assert!(Avx2::is_supported());
            algo::gfmul(self.0, y)
        }
    }
}

impl Zeroed for H {
    #[inline(always)]
    fn zeroed() -> Self {
        Self(__m128i::zeroed())
    }
}

impl HKey for H {
    #[inline(always)]
    fn new(mut h: __m128i) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());

            h = h.reverse();
            h = algo::init(h);

            Self(h)
        }
    }

    #[inline(always)]
    fn derive(&self, first: &Self) -> Self {
        Self(self.mul(first.0))
    }

    #[inline(always)]
    fn h(&self) -> __m128i {
        self.0
    }

    #[inline(always)]
    fn r(&self) -> __m128i {
        unsafe {
            debug_assert!(Avx2::is_supported());

            let h = self.0;
            let r = _mm_shuffle_epi32(h, 78);
            r.xor(h)
        }
    }
}

#[derive(Clone, Copy)]
pub struct Hr {
    h: H,
    r: __m128i,
}

impl Zeroed for Hr {
    #[inline(always)]
    fn zeroed() -> Self {
        Self {
            h: H::zeroed(),
            r: __m128i::zeroed(),
        }
    }
}

impl HKey for Hr {
    #[inline(always)]
    fn new(h: __m128i) -> Self {
        let h = H::new(h);
        let r = h.r();
        Self { h, r }
    }

    #[inline(always)]
    fn derive(&self, first: &Self) -> Self {
        let h = self.h.derive(&first.h);
        let r = h.r();
        Self { h, r }
    }

    #[inline(always)]
    fn h(&self) -> __m128i {
        self.h.h()
    }

    #[inline(always)]
    fn r(&self) -> __m128i {
        self.r
    }
}
