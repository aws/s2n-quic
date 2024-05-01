// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aead::Aead, header_key::HeaderKey, hkdf, iv, ring_aead as aead};
use core::fmt;
use s2n_quic_core::{
    assume,
    crypto::{label, packet_protection, scatter},
};
use zeroize::{Zeroize, Zeroizing};

mod negotiated;
mod ring;
use self::ring as platform;

pub use negotiated::NegotiatedCipherSuite;

macro_rules! impl_cipher_suite {
    (
        $name:ident,
        $lower:ident,
        $digest:path,
        $cipher:path,
        $cipher_key_len:expr,
        $header_protection:path,
        $key_label:expr,
        $iv_label:expr,
        $hp_label:expr,
        $key_update_label:expr,
        $confidentiality_limit:expr,
        $integrity_limit:expr,
        $test_name:ident
    ) => {
        mod $lower {
            use super::*;

            pub const KEY_LEN: usize = $cipher_key_len;
            pub const TAG_LEN: usize = 16;
            pub const NONCE_LEN: usize = crate::ring_aead::NONCE_LEN;

            type Key = platform::$lower::Key;

            // ignore casing warnings in order to preserve the IANA name
            #[allow(non_camel_case_types, clippy::all)]
            pub struct $name {
                secret: hkdf::Prk,
                iv: iv::Iv,
                key: Key,
            }

            impl $name {
                pub fn new(secret: hkdf::Prk) -> (Self, HeaderKey) {
                    let iv = Self::new_iv(&secret);
                    let key = {
                        let secret = Self::new_key_secret(&secret);
                        Key::new(&*secret)
                    };
                    let header_key = Self::new_header_key(&secret);

                    let key = Self { secret, iv, key };

                    (key, header_key)
                }

                /// Update the cipher_suite as defined in
                /// https://www.rfc-editor.org/rfc/rfc9001#section-6
                #[inline]
                pub fn update(&self) -> Self {
                    let secret: hkdf::Prk = self
                        .secret
                        .expand(&[&$key_update_label], $digest)
                        .expect("label size verified")
                        .into();

                    let iv = Self::new_iv(&secret);
                    let key = {
                        let key = Self::new_key_secret(&secret);
                        // ask the existing key to derive the next one so it can persist any
                        // configuration
                        self.key.update(&*key)
                    };
                    Self { secret, iv, key }
                }

                #[inline]
                pub fn update_pmtu(&mut self, mtu: u16) {
                    if self.key.should_update_pmtu(mtu) {
                        let secret = Self::new_key_secret(&self.secret);
                        self.key.update_pmtu(&*secret, mtu);
                    }
                }

                fn new_key_secret(secret: &hkdf::Prk) -> Zeroizing<[u8; KEY_LEN]> {
                    let mut key = Zeroizing::new([0u8; KEY_LEN]);

                    secret
                        .expand(&[&$key_label], &$cipher)
                        .expect("label size verified")
                        .fill(&mut key.as_mut())
                        .expect("fill size verified");

                    key
                }

                fn new_iv(secret: &hkdf::Prk) -> iv::Iv {
                    iv::Iv::new(secret, &$iv_label)
                }

                fn new_header_key(secret: &hkdf::Prk) -> HeaderKey {
                    HeaderKey::new::<{ KEY_LEN }>(secret, &$hp_label, &$header_protection)
                }
            }

            impl Zeroize for $name {
                fn zeroize(&mut self) {
                    self.iv.zeroize();
                    self.key.zeroize();
                }
            }

            impl s2n_quic_core::crypto::Key for $name {
                #[inline]
                fn decrypt(
                    &self,
                    packet_number: u64,
                    header: &[u8],
                    payload: &mut [u8],
                ) -> Result<(), packet_protection::Error> {
                    let nonce = self.iv.nonce(packet_number);

                    let payload_len = payload
                        .len()
                        .checked_sub(TAG_LEN)
                        .ok_or_else(|| packet_protection::Error::DECRYPT_ERROR)?;

                    let (payload, tag) = payload.split_at_mut(payload_len);
                    let tag = {
                        use core::convert::TryInto;
                        let res = (&tag[..]).try_into();
                        unsafe {
                            assume!(res.is_ok());
                        }
                        res.unwrap()
                    };

                    self.key.decrypt(&nonce, header, payload, tag)?;

                    Ok(())
                }

                #[inline]
                fn encrypt(
                    &self,
                    packet_number: u64,
                    header: &[u8],
                    payload: &mut scatter::Buffer,
                ) -> Result<(), packet_protection::Error> {
                    let nonce = self.iv.nonce(packet_number);
                    self.key.encrypt(&nonce, header, payload)?;
                    Ok(())
                }

                #[inline]
                fn tag_len(&self) -> usize {
                    $cipher.tag_len()
                }

                //= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
                //# Any TLS cipher suite that is specified for use with QUIC MUST define
                //# limits on the use of the associated AEAD function that preserves
                //# margins for confidentiality and integrity.
                #[inline]
                fn aead_confidentiality_limit(&self) -> u64 {
                    $confidentiality_limit
                }

                #[inline]
                fn aead_integrity_limit(&self) -> u64 {
                    $integrity_limit
                }

                #[inline]
                fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
                    s2n_quic_core::crypto::tls::CipherSuite::$name
                }
            }

            impl fmt::Debug for $name {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.debug_struct(stringify!($name)).finish()
                }
            }

            impl Drop for $name {
                fn drop(&mut self) {
                    self.zeroize();
                }
            }

            #[test]
            fn $test_name() {
                fn compute_vec_label(len: usize, label: &[u8]) -> Vec<u8> {
                    let mut out = vec![];
                    label::compute_label(len, label, &mut out);
                    out
                }

                insta::assert_debug_snapshot!(
                    concat!("integrity_", stringify!($test_name)),
                    $integrity_limit
                );
                insta::assert_debug_snapshot!(
                    concat!("confidentiality_", stringify!($test_name)),
                    $confidentiality_limit
                );

                assert_eq!(KEY_LEN, $cipher.key_len(), "key len mismatch");

                assert_eq!(
                    compute_vec_label($cipher.key_len(), b"quic key"),
                    $key_label,
                    "key label mismatch"
                );

                assert_eq!(
                    compute_vec_label(iv::NONCE_LEN, b"quic iv"),
                    $iv_label,
                    "iv label mismatch"
                );

                assert_eq!(
                    compute_vec_label($header_protection.key_len(), b"quic hp"),
                    $hp_label,
                    "hp label mismatch"
                );

                assert_eq!(
                    compute_vec_label(
                        $digest.hmac_algorithm().digest_algorithm().output_len,
                        b"quic ku"
                    ),
                    $key_update_label,
                    "key update label mismatch"
                );
            }
        }

        pub use $lower::$name;
    };
}

//= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
//# For AEAD_AES_128_GCM and AEAD_AES_256_GCM, the confidentiality limit
//# is 2^23 encrypted packets; see Appendix B.1.
impl_cipher_suite!(
    TLS_AES_256_GCM_SHA384,
    aes256_gcm,
    hkdf::HKDF_SHA384,
    aead::AES_256_GCM,
    256 / 8, // 256-bit key
    aead::quic::AES_256,
    label::QUIC_KEY_32,
    label::QUIC_IV_12,
    label::QUIC_HP_32,
    label::QUIC_KU_48,
    u64::pow(2, 23), // Confidentiality limit
    u64::pow(2, 52), // Integrity limit
    tls_aes_256_gcm_sha384_test
);

//= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
//# For
//# AEAD_CHACHA20_POLY1305, the confidentiality limit is greater than the
//# number of possible packets (2^62) and so can be disregarded.
impl_cipher_suite!(
    TLS_CHACHA20_POLY1305_SHA256,
    chacha20_poly1305,
    hkdf::HKDF_SHA256,
    aead::CHACHA20_POLY1305,
    256 / 8, // 256-bit key
    aead::quic::CHACHA20,
    label::QUIC_KEY_32,
    label::QUIC_IV_12,
    label::QUIC_HP_32,
    label::QUIC_KU_32,
    u64::pow(2, 62), // Confidentiality limit even though specification notes it can be disregarded
    u64::pow(2, 36), // Integrity limit
    tls_chacha20_poly1305_sha256_test
);

// See above annotation regarding AEAD_AES_128 and AEAD_AES_256
impl_cipher_suite!(
    TLS_AES_128_GCM_SHA256,
    aes128_gcm,
    hkdf::HKDF_SHA256,
    aead::AES_128_GCM,
    128 / 8, // 128-bit key
    aead::quic::AES_128,
    label::QUIC_KEY_16,
    label::QUIC_IV_12,
    label::QUIC_HP_16,
    label::QUIC_KU_32,
    u64::pow(2, 23), // Confidentiality limit
    u64::pow(2, 52), // Integrity limit
    tls_aes_128_gcm_sha256_test
);
