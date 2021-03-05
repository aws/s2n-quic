use crate::crypto::CryptoError;

/// A trait for crypto keys
pub trait Key {
    /// Decrypt a payload
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError>;

    /// Encrypt a payload
    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError>;

    /// Length of the appended tag
    fn tag_len(&self) -> usize;

    /// Maximum number of packets a key can encrypt
    fn aead_confidentiality_limit(&self) -> u64;

    /// Maximum number of decryption failures allowed for a ciphersuite
    fn aead_integrity_limit(&self) -> u64;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::crypto::{
        retry::{IntegrityTag, INTEGRITY_TAG_LEN},
        CryptoError, HandshakeCrypto, HeaderCrypto, HeaderProtectionMask, InitialCrypto,
        OneRTTCrypto, RetryCrypto, ZeroRTTCrypto,
    };

    #[derive(Debug)]
    pub struct Key {
        pub confidentiality_limit: u64,
        pub integrity_limit: u64,
        pub derivations: u64,
        pub fail_on_decrypt: bool,
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
        ) -> Result<(), CryptoError> {
            if self.fail_on_decrypt {
                return Err(CryptoError::DECRYPT_ERROR);
            }

            Ok(())
        }

        /// Encrypt a payload
        fn encrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
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
    }

    impl HeaderCrypto for Key {
        fn opening_header_protection_mask(
            &self,
            _ciphertext_sample: &[u8],
        ) -> HeaderProtectionMask {
            Default::default()
        }

        fn opening_sample_len(&self) -> usize {
            0
        }

        fn sealing_header_protection_mask(
            &self,
            _ciphertext_sample: &[u8],
        ) -> HeaderProtectionMask {
            Default::default()
        }

        fn sealing_sample_len(&self) -> usize {
            0
        }
    }

    impl InitialCrypto for Key {
        fn new_server(_connection_id: &[u8]) -> Self {
            Key::default()
        }

        fn new_client(_connection_id: &[u8]) -> Self {
            Key::default()
        }
    }
    impl HandshakeCrypto for Key {}
    impl OneRTTCrypto for Key {
        fn derive_next_key(&self) -> Self {
            Self {
                integrity_limit: self.integrity_limit,
                confidentiality_limit: self.confidentiality_limit,
                derivations: self.derivations + 1,
                fail_on_decrypt: self.fail_on_decrypt,
            }
        }
    }
    impl ZeroRTTCrypto for Key {}
    impl RetryCrypto for Key {
        fn generate_tag(_payload: &[u8]) -> IntegrityTag {
            [0u8; INTEGRITY_TAG_LEN]
        }
        fn validate(_payload: &[u8], _tag: IntegrityTag) -> Result<(), CryptoError> {
            Ok(())
        }
    }
}
