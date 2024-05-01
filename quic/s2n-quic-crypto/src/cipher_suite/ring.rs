// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! fips_supported_key {
    ($name:ident, $ring_cipher:path, $key_size:expr, $tag_len:expr) => {
        pub mod $name {
            use super::super::$name::{KEY_LEN, NONCE_LEN, TAG_LEN};
            use crate::ring_aead::{self as aead};
            use s2n_quic_core::crypto::scatter;
            use zeroize::Zeroize;

            pub struct Key {
                #[cfg(feature = "fips")]
                key: crate::aead::fips::FipsKey,
                #[cfg(not(feature = "fips"))]
                key: aead::LessSafeKey,
            }

            impl Key {
                #[cfg(feature = "fips")]
                pub fn new(secret: &[u8; KEY_LEN]) -> Self {
                    let key = crate::aead::fips::FipsKey::new(&$ring_cipher, secret)
                        .expect("key successfully created");
                    Self { key }
                }

                #[cfg(not(feature = "fips"))]
                pub fn new(secret: &[u8; KEY_LEN]) -> Self {
                    let unbound_key =
                        aead::UnboundKey::new(&$ring_cipher, secret).expect("key size verified");
                    let key = aead::LessSafeKey::new(unbound_key);
                    Self { key }
                }
            }

            key_impl!($name, $ring_cipher, $key_size, $tag_len);
        }
    };
}

macro_rules! key {
    ($name:ident, $ring_cipher:path, $key_size:expr, $tag_len:expr) => {
        pub mod $name {
            use super::super::$name::{KEY_LEN, NONCE_LEN, TAG_LEN};
            use crate::ring_aead::{self as aead};
            use s2n_quic_core::crypto::scatter;
            use zeroize::Zeroize;

            pub struct Key {
                key: aead::LessSafeKey,
            }

            impl Key {
                pub fn new(secret: &[u8; KEY_LEN]) -> Self {
                    let unbound_key =
                        aead::UnboundKey::new(&$ring_cipher, secret).expect("key size verified");
                    let key = aead::LessSafeKey::new(unbound_key);
                    Self { key }
                }
            }

            key_impl!($name, $ring_cipher, $key_size, $tag_len);
        }
    };
}

macro_rules! key_impl {
    ($name:ident, $ring_cipher:path, $key_size:expr, $tag_len:expr) => {
        impl Key {
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
                &mut self,
                nonce: &[u8; NONCE_LEN],
                aad: &[u8],
                payload: &mut scatter::Buffer,
            ) -> crate::aead::Result {
                self.key.encrypt(nonce, aad, payload)
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
    };
}

fips_supported_key!(aes128_gcm, aead::AES_128_GCM, 128 / 8, 16);
fips_supported_key!(aes256_gcm, aead::AES_256_GCM, 256 / 8, 16);
// Don't create a FipsKey for CHACHA20_POLY1305 since TlsRecordSealingKey and
// TlsRecordOpeningKey don't support CHACHA20_POLY1305
key!(chacha20_poly1305, aead::CHACHA20_POLY1305, 256 / 8, 16);
