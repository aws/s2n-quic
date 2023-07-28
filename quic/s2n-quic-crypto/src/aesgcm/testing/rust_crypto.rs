// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aead::{self, scatter},
    aesgcm::{testing::NONCE_LEN, TAG_LEN},
};
use aes_gcm::{AeadInPlace, Aes128Gcm, Aes256Gcm, KeyInit};
use s2n_codec::Encoder;

macro_rules! impl_aes {
    ($name:ident, $lower:ident) => {
        impl aead::Aead for $name {
            type Nonce = [u8; NONCE_LEN];
            type Tag = [u8; TAG_LEN];

            fn encrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                payload: &mut scatter::Buffer,
            ) -> aead::Result {
                let buffer = payload.flatten();
                let tag = {
                    let (input, _) = buffer.split_mut();
                    self.encrypt_in_place_detached(nonce.into(), aad, input)
                        .map_err(|_| aead::Error::INTERNAL_ERROR)?
                };
                buffer.write_slice(&tag);
                Ok(())
            }

            fn decrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                input: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> aead::Result {
                self.decrypt_in_place_detached(nonce.into(), aad, input, tag.into())
                    .map_err(|_| aead::Error::DECRYPT_ERROR)?;
                Ok(())
            }
        }

        pub mod $lower {
            use super::*;
            use crate::aesgcm::testing::$lower::Implementation;

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

impl_aes!(Aes128Gcm, aes128);
impl_aes!(Aes256Gcm, aes256);
