// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    arch::*,
    block::{Batch, BatchMut, Block, Zeroed},
};
use core::mem::size_of;

pub const LEN: usize = size_of::<__m128i>();

impl Block for __m128i {
    #[inline(always)]
    fn from_array(block: [u8; LEN]) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            _mm_loadu_si128(block.as_ptr() as *const _)
        }
    }

    #[inline(always)]
    fn into_array(self) -> [u8; LEN] {
        unsafe { core::mem::transmute(self) }
    }

    #[inline(always)]
    fn xor(self, x: Self) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            _mm_xor_si128(self, x)
        }
    }
}

impl Batch for __m128i {
    type Block = __m128i;

    #[inline(always)]
    fn for_each<F: FnMut(usize, &__m128i)>(&self, mut f: F) {
        f(0, self);
    }
}

impl BatchMut for __m128i {
    #[inline(always)]
    fn update<F: FnMut(usize, &mut __m128i)>(&mut self, mut f: F) {
        f(0, self);
    }
}

impl Zeroed for __m128i {
    #[inline(always)]
    fn zeroed() -> Self {
        unsafe { core::mem::transmute([0u8; 16]) }
    }
}

pub trait M128iExt {
    fn reverse(self) -> Self;
    fn from_slice(bytes: &[u8]) -> Self;
    fn into_slice(self, bytes: &mut [u8]);
    fn mask(self, len: usize) -> Self;
}

impl M128iExt for __m128i {
    #[inline(always)]
    fn reverse(self) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());

            let mask: [u8; 16] = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
            let mask = Self::from_array(mask);
            _mm_shuffle_epi8(self, mask)
        }
    }

    #[inline(always)]
    fn from_slice(bytes: &[u8]) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());

            let mut array = [0u8; LEN];
            copy_128(bytes.as_ptr(), array.as_mut_ptr(), bytes.len());
            Self::from_array(array)
        }
    }

    #[inline(always)]
    fn mask(self, len: usize) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            unsafe_assert!(0 < len && len < LEN);

            // compute a mask that can be shifted to only include a `len` of bytes
            const MASK: [u8; 31] = {
                let mut mask = [0u8; 31];
                let mut idx = 0;
                // only fill in the first `LEN` bytes
                while idx < LEN {
                    mask[idx] = 0xff;
                    idx += 1;
                }
                mask
            };

            let offset = MASK.get_unchecked(LEN - len);
            let mask = _mm_loadu_si128(offset as *const _ as *const _);

            _mm_and_si128(self, mask)
        }
    }

    #[inline(always)]
    fn into_slice(self, bytes: &mut [u8]) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            unsafe_assert!(bytes.len() <= LEN);
            copy_128(
                &self as *const _ as *const u8,
                bytes.as_mut_ptr(),
                bytes.len(),
            );
        }
    }
}

/// Copies up to 16 bytes from `from` into `to`
///
/// This exists to avoid having to call memcpy
#[inline(always)]
unsafe fn copy_128(mut from: *const u8, mut to: *mut u8, mut len: usize) {
    macro_rules! copy {
        ($($len:expr),*) => {
            $(
                if let Some(next) = len.checked_sub($len) {
                    len = next;
                    *(to as *mut [u8; $len]) = *(from as *const [u8; $len]);
                    from = from.add($len);
                    to = to.add($len);
                }
            )*
        }
    }

    copy!(128, 64, 32, 16, 8, 4, 2, 1);
    let _ = from;
    let _ = to;
    let _ = len;
}

#[test]
fn copy_128_test() {
    for i in 0..LEN {
        dbg!(i);

        let mut expected = [1u8; LEN];

        let mut source = [0u8; LEN];
        for (a, b) in source.iter_mut().zip(&mut expected).take(i) {
            *a = 2;
            *b = 2;
        }

        let mut dest = [1u8; LEN];
        unsafe {
            copy_128(source.as_ptr(), dest.as_mut_ptr(), i);
        }

        assert_eq!(dest, expected);
    }
}
