// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::block::{Batch, Block};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const TAG_LEN: usize = 16;
#[cfg(any(target_arch = "x86", target_arch = "x86_64", test))]
pub const KEY_LEN: usize = 16;

pub trait Constructor {
    type GHash: GHash;

    #[cfg(any(target_arch = "x86", target_arch = "x86_64", test))]
    fn create(&self, key: [u8; KEY_LEN]) -> Self::GHash;
}

pub trait GHash {
    type State;
    type Block: Block;

    fn start(&self, required_blocks: usize) -> Self::State;
    fn update<B: Batch<Block = Self::Block>>(&self, state: &mut Self::State, block: &B);
    fn finish(&self, state: Self::State) -> Self::Block;
}

#[cfg(test)]
mod tests;
