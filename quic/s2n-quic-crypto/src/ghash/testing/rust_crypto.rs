// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    block::LEN as BLOCK_LEN,
    ghash::testing::{GHash, Implementation},
};
use core::convert::TryInto;
use ghash::{
    universal_hash::{NewUniversalHash, UniversalHash},
    GHash as Impl,
};

impl GHash for Impl {
    fn hash(&self, input: &[u8]) -> [u8; BLOCK_LEN] {
        let mut state = self.clone();
        for block in input.chunks_exact(BLOCK_LEN) {
            let block: [u8; BLOCK_LEN] = block.try_into().unwrap();
            state.update(&block.into());
        }
        state.finalize().into_bytes().into()
    }
}

pub fn implementations(mut impls: Vec<Implementation>) -> Vec<Implementation> {
    impls.push(Implementation {
        name: "RustCrypto",
        new: |key| Box::new(Impl::new(&key.into())),
    });
    impls
}
