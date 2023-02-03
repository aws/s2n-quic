// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::aes::BLOCK_LEN;
pub use crate::{aes::testing as aes, aesgcm::testing as aesgcm, ghash::testing as ghash};
use core::{fmt, ops::Deref};

pub const BLOCK_SIZES: &[Block] = &[
    Block(1),
    Block(6),
    Block(6 * 2),
    Block(6 * 4),
    Block(6 * 8),
    Block(6 * 12),
    Block(6 * 16),
];

pub const MAX_PAYLOAD: usize = 6 * BLOCK_LEN * 16;
pub const MAX_BLOCKS: usize = (MAX_PAYLOAD / BLOCK_LEN) + 2;

static PAYLOAD: &[u8] = &[123; MAX_PAYLOAD];

#[derive(Clone, Copy, Debug)]
pub struct Block(usize);

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(f, "blocks_{b:0>2}")
    }
}

impl Deref for Block {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &PAYLOAD[..(self.0 * BLOCK_LEN)]
    }
}

#[cfg(test)]
pub(crate) struct Outcome<O> {
    pub name: &'static str,
    pub output: O,
}

#[cfg(test)]
impl<O: AsRef<[u8]>> fmt::Debug for Outcome<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use pretty_hex::PrettyHex;

        write!(f, "{}: {}", self.output.as_ref().hex_dump(), self.name)
    }
}
