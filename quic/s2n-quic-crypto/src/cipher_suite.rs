// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aead::Aead, header_key::HeaderKey, iv};
use ::ring::{aead, hkdf};
use core::fmt;
use s2n_quic_core::crypto::{label, CryptoError};
use zeroize::{Zeroize, Zeroizing};

mod negotiated;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[path = "cipher_suite/x86.rs"]
mod platform;
mod ring;
use s2n_quic_core::crypto::application::limited::MAX_MTU;

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
use self::ring as platform;

pub use negotiated::NegotiatedCipherSuite;

/// If packet size is limited, then the confidentiality limit can be increased.
#[derive(Debug)]
struct RestrictedAeadConfidentialityLimit {
    pub packet_size_limit: u64,
    pub confidentiality_limit: u64,
}

impl RestrictedAeadConfidentialityLimit {
    pub const fn new(packet_size_limit: u64, confidentiality_limit: u64) -> Self {
        Self {
            packet_size_limit,
            confidentiality_limit,
        }
    }
}

#[derive(Debug)]
struct AeadConfidentialityLimit {
    pub default_confidentiality_limit: u64,
    pub restricted_confidentiality_limit: Option<RestrictedAeadConfidentialityLimit>,
}

impl AeadConfidentialityLimit {
    pub const fn new(
        default_confidentiality_limit: u64,
        restricted_confidentiality_limit: Option<RestrictedAeadConfidentialityLimit>,
    ) -> Self {
        Self {
            default_confidentiality_limit,
            restricted_confidentiality_limit,
        }
    }
}

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
            pub const NONCE_LEN: usize = crate::aesgcm::NONCE_LEN;

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
                ) -> Result<(), CryptoError> {
                    let nonce = self.iv.nonce(packet_number);

                    let payload_len = payload
                        .len()
                        .checked_sub(TAG_LEN)
                        .ok_or_else(|| CryptoError::DECRYPT_ERROR)?;

                    let (payload, tag) = payload.split_at_mut(payload_len);
                    let tag = {
                        use core::convert::TryInto;
                        let res = (&tag[..]).try_into();
                        unsafe {
                            unsafe_assert!(res.is_ok());
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
                    payload: &mut [u8],
                ) -> Result<(), CryptoError> {
                    let nonce = self.iv.nonce(packet_number);

                    let payload_len = payload
                        .len()
                        .checked_sub(TAG_LEN)
                        .ok_or_else(|| CryptoError::DECRYPT_ERROR)?;

                    let (payload, tag) = payload.split_at_mut(payload_len);
                    let tag = {
                        use core::convert::TryInto;
                        let res = tag.try_into();
                        unsafe {
                            unsafe_assert!(res.is_ok());
                        }
                        res.unwrap()
                    };

                    self.key.encrypt(&nonce, header, payload, tag)?;

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
                    if let Some(restriction) =
                        $confidentiality_limit.restricted_confidentiality_limit
                    {
                        let max_mtu: u16 = MAX_MTU.lock().unwrap().clone().into();
                        if (max_mtu as u64) <= restriction.packet_size_limit {
                            return restriction.confidentiality_limit;
                        }
                    }
                    $confidentiality_limit.default_confidentiality_limit
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

//= https://www.rfc-editor.org/rfc/rfc9001#name-analysis-of-aead_aes_128_gc
//# Endpoints that do not send packets larger than 2^11 bytes cannot protect more than
//# 2^28 packets in a single connection without causing an attacker to gain a more significant
// advantage than the target of 2^-57.
const AEAD_AES_GCM_CONF_LIMIT_RESTRICTION: RestrictedAeadConfidentialityLimit =
    RestrictedAeadConfidentialityLimit::new(u64::pow(2, 11), u64::pow(2, 28));

//= https://www.rfc-editor.org/rfc/rfc9001#name-limits-on-aead-usage
//# For AEAD_AES_128_GCM and AEAD_AES_256_GCM, the confidentiality limit is 2^23 encrypted
//# packets; see Appendix B.1.
const AEAD_AES_GCM_CONFIDENTIALITY_LIMIT: AeadConfidentialityLimit =
    AeadConfidentialityLimit::new(u64::pow(2, 23), Some(AEAD_AES_GCM_CONF_LIMIT_RESTRICTION));

//= https://www.rfc-editor.org/rfc/rfc9001#name-limits-on-aead-usage
//# For AEAD_CHACHA20_POLY1305, the confidentiality limit is greater than the number of
//# possible packets (2^62) and so can be disregarded.
const AEAD_CHACHA20_CONFIDENTIALITY_LIMIT: AeadConfidentialityLimit =
    AeadConfidentialityLimit::new(u64::pow(2, 62), None);

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
    AEAD_AES_GCM_CONFIDENTIALITY_LIMIT,
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
    AEAD_CHACHA20_CONFIDENTIALITY_LIMIT,
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
    AEAD_AES_GCM_CONFIDENTIALITY_LIMIT,
    u64::pow(2, 52), // Integrity limit
    tls_aes_128_gcm_sha256_test
);

// Disabling warnings; Key and global functions required for these tests
#[allow(unused_imports)]
mod tests {
    use super::*;
    use s2n_quic_core::crypto::limited::set_global_max_mtu;
    use s2n_quic_core::crypto::Key;

    #[test]
    fn tls_aead_limit_tests() {
        let previous_global_max_mtu: u16 = MAX_MTU.lock().unwrap().clone().into();

        let algorithm: hkdf::Algorithm = hkdf::HKDF_SHA384;
        let prk = hkdf::Prk::new_less_safe(algorithm, &[]);
        let (aes_256, _) = TLS_AES_256_GCM_SHA384::new(prk.clone());
        let (aes_128, _) = TLS_AES_128_GCM_SHA256::new(prk.clone());
        let (chacha_20, _) = TLS_CHACHA20_POLY1305_SHA256::new(prk.clone());

        // setting the mtu right below the limit
        set_global_max_mtu(u16::pow(2, 11) - 1);
        assert_eq!(aes_256.aead_confidentiality_limit(), u64::pow(2, 28));
        assert_eq!(aes_128.aead_confidentiality_limit(), u64::pow(2, 28));
        assert_eq!(chacha_20.aead_confidentiality_limit(), u64::pow(2, 62));

        // setting the mtu right at the limit
        set_global_max_mtu(u16::pow(2, 11));
        assert_eq!(aes_256.aead_confidentiality_limit(), u64::pow(2, 28));
        assert_eq!(aes_128.aead_confidentiality_limit(), u64::pow(2, 28));
        assert_eq!(chacha_20.aead_confidentiality_limit(), u64::pow(2, 62));

        // setting the mtu right above the limit
        set_global_max_mtu(u16::pow(2, 11) + 1);
        assert_eq!(aes_256.aead_confidentiality_limit(), u64::pow(2, 23));
        assert_eq!(aes_128.aead_confidentiality_limit(), u64::pow(2, 23));
        assert_eq!(chacha_20.aead_confidentiality_limit(), u64::pow(2, 62));

        // Restore the global max mtu
        set_global_max_mtu(previous_global_max_mtu);
        assert_eq!(aes_256.aead_integrity_limit(), u64::pow(2, 52));
        assert_eq!(aes_128.aead_integrity_limit(), u64::pow(2, 52));
        assert_eq!(chacha_20.aead_integrity_limit(), u64::pow(2, 36));
    }
}
