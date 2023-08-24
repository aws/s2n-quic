// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::ring;

macro_rules! aesgcm {
    ($name:ident, $cipher:ident) => {
        pub mod $name {
            use super::{
                super::$name::{KEY_LEN, NONCE_LEN, TAG_LEN},
                ring,
            };
            use crate::{
                aead::{self, scatter},
                aes::{
                    x86::$cipher::{EncryptionKey, Key as AesKey},
                    $cipher::Key as Wrapper,
                    Encrypt,
                },
                aesgcm::generic::AesGcm,
                arch::*,
                block::{Block, Zeroed as _, LEN as BLOCK_LEN},
                ctr::x86::Ctr,
                ghash::x86::{hkey, precomputed},
            };
            use zeroize::{Zeroize, ZeroizeOnDrop};

            // Even though the ring variant is quite large, it's not worth allocating since we will
            // likely allocate the precomputed table after a PMTU update.
            #[allow(clippy::large_enum_variant)]
            #[derive(Zeroize)]
            pub enum Key {
                Precomputed(PrecomputedKey),
                Ring(ring::$name::Key),
            }

            impl Key {
                #[inline]
                pub fn new(secret: &[u8; KEY_LEN]) -> Self {
                    // default to the ring implementation until the PMTU changes
                    let key = ring::$name::Key::new(secret);
                    Self::Ring(key)
                }

                pub fn should_update_pmtu(&self, mtu: u16) -> bool {
                    match self {
                        // if the precomputed key isn't supported, do nothing
                        _ if !Avx2::is_supported() => false,
                        // if we've already precomputed a larger key, do nothing
                        Self::Precomputed(key) if key.mtu >= mtu => false,
                        _ => true,
                    }
                }

                pub fn update(&self, secret: &[u8; KEY_LEN]) -> Self {
                    match self {
                        Self::Ring(_) => Self::new(secret),
                        Self::Precomputed(key) => Self::Precomputed(unsafe {
                            debug_assert!(Avx2::is_supported());
                            PrecomputedKey::new(secret, key.mtu)
                        }),
                    }
                }

                pub fn update_pmtu(&mut self, secret: &[u8; KEY_LEN], mtu: u16) {
                    debug_assert!(self.should_update_pmtu(mtu));

                    *self = Self::Precomputed(unsafe {
                        debug_assert!(Avx2::is_supported());
                        PrecomputedKey::new(secret, mtu)
                    })
                }
            }

            impl aead::Aead for Key {
                type Nonce = [u8; NONCE_LEN];
                type Tag = [u8; TAG_LEN];

                #[inline]
                fn encrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    payload: &mut scatter::Buffer,
                ) -> aead::Result {
                    match self {
                        Self::Precomputed(key) => key.encrypt(nonce, aad, payload),
                        Self::Ring(key) => key.encrypt(nonce, aad, payload),
                    }
                }

                #[inline]
                fn decrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    input: &mut [u8],
                    tag: &[u8; TAG_LEN],
                ) -> aead::Result {
                    match self {
                        Self::Precomputed(key) => key.decrypt(nonce, aad, input, tag),
                        Self::Ring(key) => key.decrypt(nonce, aad, input, tag),
                    }
                }
            }

            type PrecomputedGHash = precomputed::Allocated<hkey::H>;
            type PrecomputedAesGcmKey = AesGcm<Wrapper<EncryptionKey>, PrecomputedGHash, Ctr, 6>;

            #[derive(Zeroize, ZeroizeOnDrop)]
            pub struct PrecomputedKey {
                key: PrecomputedAesGcmKey,
                mtu: u16,
            }

            impl PrecomputedKey {
                #[inline]
                #[target_feature(enable = "aes,avx2,pclmulqdq")]
                unsafe fn new(secret: &[u8; KEY_LEN], mtu: u16) -> Self {
                    debug_assert!(Avx2::is_supported());
                    let AesKey { encrypt, .. } = AesKey::new(*secret);
                    let key = Wrapper(encrypt);
                    let mut ghash_key = __m128i::zeroed();
                    key.encrypt(&mut ghash_key);
                    // round up to the next block size
                    let blocks = (mtu as usize + BLOCK_LEN - 1) / BLOCK_LEN;
                    let ghash = PrecomputedGHash::new(ghash_key.into_array(), blocks);
                    let key = AesGcm::new(key, ghash);
                    Self { key, mtu }
                }

                #[inline]
                #[target_feature(enable = "aes,avx2,pclmulqdq")]
                unsafe fn encrypt_impl(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    payload: &mut scatter::Buffer,
                ) -> aead::Result {
                    aead::Aead::encrypt(&self.key, nonce, aad, payload)
                }

                #[inline]
                #[target_feature(enable = "aes,avx2,pclmulqdq")]
                unsafe fn decrypt_impl(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    input: &mut [u8],
                    tag: &[u8; TAG_LEN],
                ) -> aead::Result {
                    aead::Aead::decrypt(&self.key, nonce, aad, input, tag)
                }
            }

            impl aead::Aead for PrecomputedKey {
                type Nonce = [u8; NONCE_LEN];
                type Tag = [u8; TAG_LEN];

                #[inline]
                fn encrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    payload: &mut scatter::Buffer,
                ) -> aead::Result {
                    unsafe {
                        debug_assert!(Avx2::is_supported());
                        self.encrypt_impl(nonce, aad, payload)
                    }
                }

                #[inline]
                fn decrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    input: &mut [u8],
                    tag: &[u8; TAG_LEN],
                ) -> aead::Result {
                    unsafe {
                        debug_assert!(Avx2::is_supported());
                        self.decrypt_impl(nonce, aad, input, tag)
                    }
                }
            }
        }
    };
}

aesgcm!(aes128_gcm, aes128);
aesgcm!(aes256_gcm, aes256);

// re-export chacha until it's implemented in this crate
pub use super::ring::chacha20_poly1305;
