// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::ghash::TAG_LEN;
use lazy_static::lazy_static;

pub struct Implementation {
    pub(crate) name: &'static str,
    pub(crate) new: fn(key: [u8; 16]) -> Box<dyn GHash>,
}

impl Implementation {
    pub fn name(&self) -> &'static str {
        self.name
    }

    #[allow(clippy::new_ret_no_self)]
    pub fn new(&self, key: [u8; 16]) -> Box<dyn GHash> {
        (self.new)(key)
    }
}

pub trait GHash {
    fn hash(&self, input: &[u8]) -> [u8; TAG_LEN];
}

lazy_static! {
    static ref IMPLEMENTATIONS: Vec<Implementation> = {
        let mut impls = vec![];

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        super::x86::testing::implementations(&mut impls);

        #[cfg(test)]
        rust_crypto::implementations(&mut impls);

        impls
    };
}

pub fn implementations() -> &'static [Implementation] {
    &*IMPLEMENTATIONS
}

#[cfg(test)]
mod rust_crypto;
