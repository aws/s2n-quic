// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! key {
    ($name:ident, $ring_cipher:path, $key_size:expr, $tag_len:expr) => {
        pub mod $name {
            use super::super::$name::{KEY_LEN, NONCE_LEN, TAG_LEN};
            use crate::ring_aead::{self as aead, LessSafeKey, UnboundKey};
            use zeroize::Zeroize;

            pub struct Key {
                key: LessSafeKey,
            }

            impl Key {
                #[inline]
                pub fn new(secret: &[u8; KEY_LEN]) -> Self {
                    let unbound_key =
                        UnboundKey::new(&$ring_cipher, secret).expect("key size verified");
                    let key = LessSafeKey::new(unbound_key);
                    Self { key }
                }

                #[inline]
                #[allow(dead_code)] // this is to maintain compatibility between implementations
                pub fn should_update_pmtu(&self, _mtu: u16) -> bool {
                    // ring doesn't implement precomputed tables
                    false
                }

                #[inline]
                #[allow(dead_code)] // this is to maintain compatibility between implementations
                pub fn update(&self, secret: &[u8; KEY_LEN]) -> Self {
                    // no state to persist
                    Self::new(secret)
                }

                #[inline]
                #[allow(dead_code)] // this is to maintain compatibility between implementations
                pub fn update_pmtu(&mut self, _secret: &[u8; KEY_LEN], _mtu: u16) {
                    unimplemented!();
                }
            }

            impl Zeroize for Key {
                fn zeroize(&mut self) {
                    // ring doesn't provide a way to zeroize keys currently
                    // https://github.com/briansmith/ring/issues/15
                }
            }

            impl crate::aead::Aead for Key {
                type Nonce = [u8; NONCE_LEN];
                type Tag = [u8; TAG_LEN];

                #[inline]
                fn encrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    input: &mut [u8],
                    tag: &mut [u8; TAG_LEN],
                ) -> crate::aead::Result {
                    self.key.encrypt(nonce, aad, input, tag)
                }

                #[inline]
                fn decrypt(
                    &self,
                    nonce: &[u8; NONCE_LEN],
                    aad: &[u8],
                    input: &mut [u8],
                    tag: &[u8; TAG_LEN],
                ) -> crate::aead::Result {
                    self.key.decrypt(nonce, aad, input, tag)
                }
            }
        }
    };
}

key!(aes128_gcm, aead::AES_128_GCM, 128 / 8, 16);
key!(aes256_gcm, aead::AES_256_GCM, 256 / 8, 16);
key!(chacha20_poly1305, aead::CHACHA20_POLY1305, 256 / 8, 16);
