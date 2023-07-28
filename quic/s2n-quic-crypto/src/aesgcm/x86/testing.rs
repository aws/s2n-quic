// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aead::{self, scatter, Aead},
    aes::Encrypt,
    aesgcm::{generic::AesGcm, NONCE_LEN, TAG_LEN},
    arch::*,
    block::{Block, Zeroed as _},
    ctr::x86::Ctr,
    ghash::x86::{hkey, precomputed::Array, GHash},
    testing::MAX_BLOCKS,
};

macro_rules! impl_target_features {
    ($name:ident, $features:literal) => {
        impl $name {
            #[inline]
            #[target_feature(enable = $features)]
            unsafe fn encrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                payload: &mut scatter::Buffer,
            ) -> aead::Result {
                self.0.encrypt(nonce, aad, payload)
            }

            #[inline]
            #[target_feature(enable = $features)]
            unsafe fn decrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                input: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> aead::Result {
                self.0.decrypt(nonce, aad, input, tag)
            }
        }

        impl aead::Aead for $name {
            type Nonce = [u8; NONCE_LEN];
            type Tag = [u8; TAG_LEN];

            fn encrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                payload: &mut scatter::Buffer,
            ) -> aead::Result {
                unsafe {
                    debug_assert!(Avx2::is_supported());
                    Self::encrypt(self, nonce, aad, payload)
                }
            }

            fn decrypt(
                &self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                input: &mut [u8],
                tag: &[u8; TAG_LEN],
            ) -> Result<(), aead::Error> {
                unsafe {
                    debug_assert!(Avx2::is_supported());
                    Self::decrypt(self, nonce, aad, input, tag)
                }
            }
        }
    };
}

macro_rules! aesgcm_impl {
    ($name:ident, $arch_name:literal, $features:literal) => {
        mod $name {
            use super::*;
            const BATCH_SIZE: usize = 6;

            pub struct Std(AesGcm<Wrapper<EncryptionKey>, GHash, Ctr, BATCH_SIZE>);
            impl_target_features!(Std, $features);

            impl Std {
                #[inline]
                #[target_feature(enable = $features)]
                pub unsafe fn new(key: [u8; KEY_LEN]) -> Self {
                    let Key { encrypt, .. } = Key::new(key);
                    let key = Wrapper(encrypt);
                    let mut ghash_key = __m128i::zeroed();
                    key.encrypt(&mut ghash_key);
                    let ghash = GHash::new(ghash_key.into_array());
                    let key = AesGcm::new(key, ghash);
                    Self(key)
                }
            }

            pub struct PreH(
                AesGcm<Wrapper<EncryptionKey>, Array<hkey::H, MAX_BLOCKS>, Ctr, BATCH_SIZE>,
            );
            impl_target_features!(PreH, $features);

            impl PreH {
                #[inline]
                #[target_feature(enable = $features)]
                pub unsafe fn new(key: [u8; KEY_LEN]) -> Self {
                    type GHash = Array<hkey::H, MAX_BLOCKS>;

                    let Key { encrypt, .. } = Key::new(key);
                    let key = Wrapper(encrypt);
                    let mut ghash_key = __m128i::zeroed();
                    key.encrypt(&mut ghash_key);
                    let ghash = GHash::new(ghash_key.into_array());
                    let key = AesGcm::new(key, ghash);
                    Self(key)
                }
            }

            pub struct PreHr(
                AesGcm<Wrapper<EncryptionKey>, Array<hkey::Hr, MAX_BLOCKS>, Ctr, BATCH_SIZE>,
            );
            impl_target_features!(PreHr, $features);

            impl PreHr {
                #[inline]
                #[target_feature(enable = $features)]
                pub unsafe fn new(key: [u8; KEY_LEN]) -> Self {
                    type GHash = Array<hkey::Hr, MAX_BLOCKS>;

                    let Key { encrypt, .. } = Key::new(key);
                    let key = Wrapper(encrypt);
                    let mut ghash_key = __m128i::zeroed();
                    key.encrypt(&mut ghash_key);
                    let ghash = GHash::new(ghash_key.into_array());
                    let key = AesGcm::new(key, ghash);
                    Self(key)
                }
            }

            pub fn implementations(impls: &mut Vec<Implementation>) {
                impls.push(Implementation {
                    name: concat!("s2n_quic/std/", $arch_name),
                    new: |key| unsafe { Box::new(Std::new(key)) },
                });

                impls.push(Implementation {
                    name: concat!("s2n_quic/pre_h/", $arch_name),
                    new: |key| unsafe { Box::new(PreH::new(key)) },
                });

                impls.push(Implementation {
                    name: concat!("s2n_quic/pre_hr/", $arch_name),
                    new: |key| unsafe { Box::new(PreHr::new(key)) },
                });
            }
        }
    };
}

macro_rules! impl_aesgcm {
    ($name:ident) => {
        pub mod $name {
            use super::*;
            use crate::{
                aes::{
                    x86::$name::{EncryptionKey, Key},
                    $name::{Key as Wrapper, KEY_LEN},
                },
                aesgcm::testing::$name::Implementation,
            };

            aesgcm_impl!(avx2, "avx2", "aes,avx2,pclmulqdq");

            pub fn implementations(impls: &mut Vec<Implementation>) {
                Avx2::call_supported(|| {
                    avx2::implementations(impls);
                });
            }
        }
    };
}

impl_aesgcm!(aes128);
impl_aesgcm!(aes256);
