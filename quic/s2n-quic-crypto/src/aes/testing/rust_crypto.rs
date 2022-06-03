// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::aes::testing::{for_each_block, Aes};
use aes::{
    cipher::{BlockDecrypt, BlockEncrypt as _, KeyInit as _},
    Aes128, Aes256,
};

macro_rules! impl_aes {
    ($name:ident, $lower:ident) => {
        impl Aes for $name {
            fn encrypt(&self, input: &mut [u8]) {
                for_each_block(input, |chunk| {
                    let mut block = aes::Block::from(*chunk);
                    self.encrypt_block(&mut block);
                    chunk.copy_from_slice(&block);
                });
            }

            fn decrypt(&self, input: &mut [u8]) {
                for_each_block(input, |chunk| {
                    let mut block = aes::Block::from(*chunk);
                    self.decrypt_block(&mut block);
                    chunk.copy_from_slice(&block);
                });
            }
        }

        pub mod $lower {
            use super::*;
            use crate::aes::testing::$lower::Implementation;

            pub fn implementations(impls: &mut Vec<Implementation>) {
                impls.push(Implementation {
                    name: "RustCrypto",
                    new: |key| {
                        let aes = $name::new(&key.into());
                        Box::new(aes)
                    },
                });
            }
        }
    };
}

impl_aes!(Aes128, aes128);
impl_aes!(Aes256, aes256);
