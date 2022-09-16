// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    block::LEN as BLOCK_LEN,
    ghash::testing::{GHash, Implementation},
};
use ghash::{
    universal_hash::{KeyInit, UniversalHash},
    GHash as Impl,
};

impl GHash for Impl {
    fn hash(&self, input: &[u8]) -> [u8; BLOCK_LEN] {
        let mut state = self.clone();
        for block in input.chunks_exact(BLOCK_LEN) {
            state.update_padded(block);
        }
        state.finalize().into()
    }
}

pub fn implementations(mut impls: Vec<Implementation>) -> Vec<Implementation> {
    impls.push(Implementation {
        name: "RustCrypto",
        new: |key| Box::new(Impl::new(&key.into())),
    });
    impls
}
