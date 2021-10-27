// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aesgcm::NONCE_LEN, block::Block};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

pub trait Ctr {
    type Block: Block;

    fn new(nonce: &[u8; NONCE_LEN]) -> Self;
    fn bit_counts(aad_len: usize, payload_len: usize) -> Self::Block;
    fn block(&self) -> Self::Block;
    fn increment(&mut self);
}
