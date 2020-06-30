use core::fmt;
use ring::{aead, hkdf};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::crypto::{label, CryptoError, HeaderCrypto, HeaderProtectionMask, Key};

pub mod negotiated;

pub use negotiated::NegotiatedCiphersuite;

struct IvLen;

impl hkdf::KeyType for IvLen {
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
        $test_name:ident
    ) => {
        #[allow(non_camel_case_types)]
        pub struct $name {
            secret: hkdf::Prk,
            iv: [u8; Self::IV_LEN],
            key: [u8; Self::KEY_LEN],
            header_key: [u8; Self::KEY_LEN],
        }

        impl $name {
            const IV_LEN: usize = aead::NONCE_LEN;
            const KEY_LEN: usize = $cipher_key_len;

            /// Create a ciphersuite with a given secret
            pub fn new(secret: hkdf::Prk) -> Self {
                let mut crypto = Self {
                    secret,
                    iv: Default::default(),
                    key: Default::default(),
                    header_key: Default::default(),
                };

                crypto.reset();

                crypto
                    .secret
                    .expand(&[&$hp_label], &$header_protection)
                    .expect("label size verified")
                    .fill(&mut crypto.header_key)
                    .expect("fill size verified");

                crypto
            }

            /// Update the ciphersuite as defined in
            /// https://tools.ietf.org/html/draft-ietf-quic-tls-24#section-6
            pub fn update(&self) -> Self {
                let secret = self
                    .secret
                    .expand(&[&$key_update_label], $digest)
                    .expect("label size verified")
                    .into();

                let mut crypto = Self {
                    secret,
                    iv: Default::default(),
                    key: Default::default(),
                    header_key: self.header_key,
                };

                crypto.reset();

                crypto
            }

            fn reset(&mut self) {
                let secret = &self.secret;

                secret
                    .expand(&[&$key_label], &$cipher)
                    .expect("label size verified")
                    .fill(&mut self.key)
                    .expect("fill size verified");

                secret
                    .expand(&[&$iv_label], IvLen)
                    .expect("label size verified")
                    .fill(&mut self.iv)
                    .expect("fill size verified");
            }

            fn aead_key(&self) -> aead::LessSafeKey {
                let unbound_key =
                    aead::UnboundKey::new(&$cipher, &self.key).expect("key size verified");
                aead::LessSafeKey::new(unbound_key)
            }

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

            fn header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
                aead::quic::HeaderProtectionKey::new(&$header_protection, &self.header_key)
                    .expect("header secret length already checked")
                    .new_mask(sample)
                    .expect("sample length already checked")
            }
        }

        impl Key for $name {
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

                self.aead_key()
                    .open_in_place(nonce, aead, payload)
                    .map_err(|_| CryptoError::decrypt_error())?;

                Ok(())
            }

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
                    .aead_key()
                    .seal_in_place_separate_tag(nonce, aead, &mut payload[..payload_len])
                    .map_err(|_| CryptoError::decrypt_error())?;

                payload[payload_len..].copy_from_slice(tagged.as_ref());

                Ok(())
            }

            fn tag_len(&self) -> usize {
                $cipher.tag_len()
            }
        }

        impl HeaderCrypto for $name {
            fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
                self.header_protection_mask(sample)
            }

            fn opening_sample_len(&self) -> usize {
                $header_protection.sample_len()
            }

            fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
                self.header_protection_mask(sample)
            }

            fn sealing_sample_len(&self) -> usize {
                $header_protection.sample_len()
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

            insta::assert_debug_snapshot!(core::mem::size_of::<$name>());

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
                compute_vec_label($cipher.key_len(), b"quic ku"),
                $key_update_label,
                "key update label mismatch"
            );
        }
    };
}

impl_ciphersuite!(
    TLS_AES_256_GCM_SHA384,
    hkdf::HKDF_SHA384,
    aead::AES_256_GCM,
    256 / 8, // 256-bit key
    aead::quic::AES_256,
    label::QUIC_KEY_32,
    label::QUIC_IV_12,
    label::QUIC_HP_32,
    label::QUIC_KU_32,
    tls_aes_256_gcm_sha384_test
);

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
    tls_chacha20_poly1305_sha256_test
);

impl_ciphersuite!(
    TLS_AES_128_GCM_SHA256,
    hkdf::HKDF_SHA256,
    aead::AES_128_GCM,
    128 / 8, // 128-bit key
    aead::quic::AES_128,
    label::QUIC_KEY_16,
    label::QUIC_IV_12,
    label::QUIC_HP_16,
    label::QUIC_KU_16,
    tls_aes_128_gcm_sha256_test
);
