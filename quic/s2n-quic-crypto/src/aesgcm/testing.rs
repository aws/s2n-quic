// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! aesgcm_impl {
    ($name:ident) => {
        pub mod $name {
            use super::*;
            use lazy_static::lazy_static;

            pub use crate::aes::$name::KEY_LEN;

            pub struct Implementation {
                pub(crate) name: &'static str,
                pub(crate) new: fn(key: [u8; KEY_LEN]) -> Key,
            }

            impl Implementation {
                pub fn name(&self) -> &'static str {
                    self.name
                }

                #[allow(clippy::new_ret_no_self)]
                pub fn new(&self, key: [u8; KEY_LEN]) -> Key {
                    (self.new)(key)
                }
            }

            lazy_static! {
                static ref IMPLEMENTATIONS: Vec<Implementation> = {
                    let mut impls = vec![];

                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    crate::aesgcm::x86::testing::$name::implementations(&mut impls);

                    crate::aesgcm::ring::$name::implementations(&mut impls);

                    #[cfg(test)]
                    super::rust_crypto::$name::implementations(&mut impls);

                    impls
                };
            }

            pub fn implementations() -> &'static [Implementation] {
                &*IMPLEMENTATIONS
            }
        }
    };
}

aesgcm_impl!(aes128);
aesgcm_impl!(aes256);

pub use crate::{
    aead::Aead,
    aesgcm::{NONCE_LEN, TAG_LEN},
};
pub type Key = Box<dyn Aead<Nonce = [u8; NONCE_LEN], Tag = [u8; TAG_LEN]>>;

#[cfg(test)]
mod rust_crypto;
