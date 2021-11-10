// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::header_key::HeaderKey;
use core::fmt;
use ring::{aead, hkdf};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::crypto::{label, CryptoError, Key};
use zeroize::Zeroizing;

mod negotiated;

pub use negotiated::NegotiatedCiphersuite;

struct IvLen;

impl hkdf::KeyType for IvLen {
    #[inline]
    fn len(&self) -> usize {
        aead::NONCE_LEN
    }
}

macro_rules! impl_ciphersuite {
    (
        $name:ident,
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
        // ignore casing warnings in order to preserve the IANA name
        #[allow(non_camel_case_types, clippy::all)]
        pub struct $name {
            secret: hkdf::Prk,
            iv: Zeroizing<[u8; Self::IV_LEN]>,
            key: aead::LessSafeKey,
        }

        impl $name {
            const IV_LEN: usize = aead::NONCE_LEN;
            const KEY_LEN: usize = $cipher_key_len;

            /// Create a ciphersuite with a given secret
            pub fn new(secret: hkdf::Prk) -> (Self, HeaderKey) {
                let iv = Self::new_iv(&secret);
                let key = Self::new_key(&secret);
                let header_key = Self::new_header_key(&secret);

                let key = Self { secret, iv, key };

                (key, header_key)
            }

            /// Update the ciphersuite as defined in
            /// https://www.rfc-editor.org/rfc/rfc9001.txt#6
            #[inline]
            pub fn update(&self) -> Self {
                let secret: hkdf::Prk = self
                    .secret
                    .expand(&[&$key_update_label], $digest)
                    .expect("label size verified")
                    .into();

                let iv = Self::new_iv(&secret);
                let key = Self::new_key(&secret);
                Self { secret, iv, key }
            }

            #[inline]
            fn generate_nonce(&self, packet_number: u64) -> [u8; Self::IV_LEN] {
                let mut nonce = [0; Self::IV_LEN];
                let mut encoder = EncoderBuffer::new(&mut nonce);

                encoder.encode(&0u32);
                encoder.encode(&packet_number);

                for (a, b) in nonce.iter_mut().zip(self.iv.iter()) {
                    *a ^= b;
                }

                nonce
            }

            fn new_key(secret: &hkdf::Prk) -> aead::LessSafeKey {
                let mut bytes = Zeroizing::new([0u8; Self::KEY_LEN]);

                secret
                    .expand(&[&$key_label], &$cipher)
                    .expect("label size verified")
                    .fill(&mut bytes.as_mut())
                    .expect("fill size verified");

                let unbound_key =
                    aead::UnboundKey::new(&$cipher, &bytes.as_ref()).expect("key size verified");
                aead::LessSafeKey::new(unbound_key)
            }

            fn new_iv(secret: &hkdf::Prk) -> Zeroizing<[u8; Self::IV_LEN]> {
                let mut bytes = [0u8; Self::IV_LEN];

                secret
                    .expand(&[&$iv_label], IvLen)
                    .expect("label size verified")
                    .fill(&mut bytes)
                    .expect("fill size verified");

                Zeroizing::new(bytes)
            }

            fn new_header_key(secret: &hkdf::Prk) -> HeaderKey {
                let mut bytes = Zeroizing::new([0u8; Self::KEY_LEN]);

                secret
                    .expand(&[&$hp_label], &$header_protection)
                    .expect("label size verified")
                    .fill(bytes.as_mut())
                    .expect("fill size verified");

                let key = aead::quic::HeaderProtectionKey::new(&$header_protection, bytes.as_ref())
                    .expect("header secret length already checked");
                HeaderKey(key)
            }
        }

        impl Key for $name {
            #[inline]
            fn decrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), CryptoError> {
                let aead = aead::Aad::from(header);
                let nonce = self.generate_nonce(packet_number);
                let nonce =
                    aead::Nonce::try_assume_unique_for_key(&nonce).expect("already verified");

                self.key
                    .open_in_place(nonce, aead, payload)
                    .map_err(|_| CryptoError::DECRYPT_ERROR)?;

                Ok(())
            }

            #[inline]
            fn encrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), CryptoError> {
                let aead = aead::Aad::from(header);
                let nonce = self.generate_nonce(packet_number);
                let nonce =
                    aead::Nonce::try_assume_unique_for_key(&nonce).expect("already verified");

                let tag_len = self.tag_len();
                let payload_len = payload.len() - tag_len;

                let tagged = self
                    .key
                    .seal_in_place_separate_tag(nonce, aead, &mut payload[..payload_len])
                    .map_err(|_| CryptoError::DECRYPT_ERROR)?;

                payload[payload_len..].copy_from_slice(tagged.as_ref());

                Ok(())
            }

            #[inline]
            fn tag_len(&self) -> usize {
                $cipher.tag_len()
            }

            //= https://www.rfc-editor.org/rfc/rfc9001.txt#6.6
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
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct(stringify!($name)).finish()
            }
        }

        #[test]
        fn $test_name() {
            fn compute_vec_label(len: usize, label: &[u8]) -> Vec<u8> {
                let mut out = vec![];
                label::compute_label(len, label, &mut out);
                out
            }

            insta::assert_debug_snapshot!(stringify!($test_name), core::mem::size_of::<$name>());
            insta::assert_debug_snapshot!(
                concat!("integrity_", stringify!($test_name)),
                $integrity_limit
            );
            insta::assert_debug_snapshot!(
                concat!("confidentiality_", stringify!($test_name)),
                $confidentiality_limit
            );

            assert_eq!($name::KEY_LEN, $cipher.key_len(), "key len mismatch");

            assert_eq!(
                compute_vec_label($cipher.key_len(), b"quic key"),
                $key_label,
                "key label mismatch"
            );

            assert_eq!(
                compute_vec_label($name::IV_LEN, b"quic iv"),
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
    };
}

//= https://www.rfc-editor.org/rfc/rfc9001.txt#6.6
//# For AEAD_AES_128_GCM and AEAD_AES_256_GCM, the confidentiality limit
//# is 2^23 encrypted packets; see Appendix B.1.
impl_ciphersuite!(
    TLS_AES_256_GCM_SHA384,
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

//= https://www.rfc-editor.org/rfc/rfc9001.txt#6.6
//# For
//# AEAD_CHACHA20_POLY1305, the confidentiality limit is greater than the
//# number of possible packets (2^62) and so can be disregarded.
impl_ciphersuite!(
    TLS_CHACHA20_POLY1305_SHA256,
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
impl_ciphersuite!(
    TLS_AES_128_GCM_SHA256,
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
