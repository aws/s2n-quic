// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

pub const LEN: usize = 16;

pub trait Block: Copy + Zeroed {
    fn from_array(array: [u8; LEN]) -> Self;
    fn into_array(self) -> [u8; LEN];
    fn xor(self, other: Self) -> Self;
    fn ct_ensure_eq(self, b: Self) -> Result<(), ()>;
}

pub trait Batch {
    type Block: Block;

    fn for_each<F: FnMut(usize, &Self::Block)>(&self, f: F);
}

pub trait BatchMut: Batch {
    fn update<F: FnMut(usize, &mut Self::Block)>(&mut self, f: F);
}

pub trait Zeroed {
    fn zeroed() -> Self;
}

macro_rules! impl_array {
    ($n:expr, [$($idx:expr),*]) => {
        impl<B: Block> Batch for [B; $n] {
            type Block = B;

            #[inline(always)]
            fn for_each<F: FnMut(usize, &Self::Block)>(&self, mut f: F) {
                $(
                    f($idx, &self[$idx]);
                )*
            }
        }

        impl<B: Block> BatchMut for [B; $n] {
            fn update<F: FnMut(usize, &mut Self::Block)>(&mut self, mut f: F) {
                $(
                    f($idx, &mut self[$idx]);
                )*
            }
        }

        impl<B: Zeroed> Zeroed for [B; $n] {
            #[inline(always)]
            fn zeroed() -> Self {
                [
                    $({
                        let _ = $idx;
                        B::zeroed()
                    }),*
                ]
            }
        }
    };
}

impl_array!(1, [0]);
impl_array!(2, [0, 1]);
impl_array!(3, [0, 1, 2]);
impl_array!(4, [0, 1, 2, 3]);
impl_array!(5, [0, 1, 2, 3, 4]);
impl_array!(6, [0, 1, 2, 3, 4, 5]);
impl_array!(7, [0, 1, 2, 3, 4, 5, 6]);
impl_array!(8, [0, 1, 2, 3, 4, 5, 6, 7]);
