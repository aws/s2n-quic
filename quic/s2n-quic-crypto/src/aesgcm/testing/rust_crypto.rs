// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::aesgcm::{
    testing::{AesGcm, NONCE_LEN},
    Error, TAG_LEN,
};
use aes_gcm::{AeadInPlace, Aes128Gcm, Aes256Gcm, NewAead};

macro_rules! impl_aes {
    ($name:ident, $lower:ident) => {
        impl AesGcm for $name {
            fn encrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                input: &mut [u8],
                tag_buf: &mut [u8; TAG_LEN],
            ) {
                let tag = self
                    .encrypt_in_place_detached(nonce.into(), aad, input)
                    .unwrap();
                tag_buf.copy_from_slice(&tag);
            }

            fn decrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                input: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> Result<(), Error> {
                self.decrypt_in_place_detached(nonce.into(), aad, input, tag.into())
                    .map_err(|_| Error)?;
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
