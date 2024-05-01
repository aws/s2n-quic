// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::packet_protection;
use s2n_codec::encoder::scatter;

/// A trait for crypto keys
pub trait Key: Send {
    /// Decrypt a payload
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), packet_protection::Error>;

    /// Encrypt a payload
    fn encrypt(
        &mut self,
        packet_number: u64,
        header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), packet_protection::Error>;

    /// Length of the appended tag
    fn tag_len(&self) -> usize;

    /// Maximum number of packets a key can encrypt
    fn aead_confidentiality_limit(&self) -> u64;

    /// Maximum number of decryption failures allowed for a cipher_suite
    fn aead_integrity_limit(&self) -> u64;

    fn cipher_suite(&self) -> crate::crypto::tls::CipherSuite;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::crypto::{
        packet_protection,
        retry::{IntegrityTag, INTEGRITY_TAG_LEN},
        scatter, HandshakeHeaderKey, HandshakeKey, HeaderKey as CryptoHeaderKey,
        HeaderProtectionMask, InitialHeaderKey, InitialKey, OneRttHeaderKey, OneRttKey, RetryKey,
        ZeroRttHeaderKey, ZeroRttKey,
    };

    #[derive(Debug)]
    pub struct Key {
        pub confidentiality_limit: u64,
        pub integrity_limit: u64,
        pub derivations: u64,
        pub fail_on_decrypt: bool,
    }

    impl Key {
        pub fn new() -> Self {
            Key::default()
        }
    }

    impl Default for Key {
        fn default() -> Self {
            // These default derivations are simply to make it easy to create this object and pass
            // tests. There is no reason for the actual derivations beyond that.
            Self {
                confidentiality_limit: 64,
                integrity_limit: 64,
                derivations: 0,
                fail_on_decrypt: false,
            }
        }
    }

    impl super::Key for Key {
        /// Decrypt a payload
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), packet_protection::Error> {
            if self.fail_on_decrypt {
                return Err(packet_protection::Error::DECRYPT_ERROR);
            }

            Ok(())
        }

        /// Encrypt a payload
        fn encrypt(
            &mut self,
            _packet_number: u64,
            _header: &[u8],
            payload: &mut scatter::Buffer,
        ) -> Result<(), packet_protection::Error> {
            // copy any bytes into the final slice
            payload.flatten();
            Ok(())
        }

        /// Length of the appended tag
        fn tag_len(&self) -> usize {
            0
        }

        fn aead_confidentiality_limit(&self) -> u64 {
            self.confidentiality_limit
        }

        fn aead_integrity_limit(&self) -> u64 {
            self.integrity_limit
        }

        fn cipher_suite(&self) -> crate::crypto::tls::CipherSuite {
            crate::crypto::tls::CipherSuite::Unknown
        }
    }

    impl InitialKey for Key {
        type HeaderKey = HeaderKey;

        fn new_server(_connection_id: &[u8]) -> (Self, Self::HeaderKey) {
            (Key::default(), HeaderKey::default())
        }

        fn new_client(_connection_id: &[u8]) -> (Self, Self::HeaderKey) {
            (Key::default(), HeaderKey::default())
        }
    }
    impl HandshakeKey for Key {}
    impl OneRttKey for Key {
        fn derive_next_key(&self) -> Self {
            Self {
                integrity_limit: self.integrity_limit,
                confidentiality_limit: self.confidentiality_limit,
                derivations: self.derivations + 1,
                fail_on_decrypt: self.fail_on_decrypt,
            }
        }

        fn update_sealer_pmtu(&mut self, _pmtu: u16) {}
        fn update_opener_pmtu(&mut self, _pmtu: u16) {}
    }
    impl ZeroRttKey for Key {}
    impl RetryKey for Key {
        fn generate_tag(_payload: &[u8]) -> IntegrityTag {
            [0u8; INTEGRITY_TAG_LEN]
        }
        fn validate(_payload: &[u8], _tag: IntegrityTag) -> Result<(), packet_protection::Error> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    pub struct HeaderKey {}

    impl HeaderKey {
        pub fn new() -> Self {
            HeaderKey::default()
        }
    }

    impl CryptoHeaderKey for HeaderKey {
        fn opening_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn opening_sample_len(&self) -> usize {
            0
        }

        fn sealing_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn sealing_sample_len(&self) -> usize {
            0
        }
    }

    impl InitialHeaderKey for HeaderKey {}
    impl HandshakeHeaderKey for HeaderKey {}
    impl OneRttHeaderKey for HeaderKey {}
    impl ZeroRttHeaderKey for HeaderKey {}
}
