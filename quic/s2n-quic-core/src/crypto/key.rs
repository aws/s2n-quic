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

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=TODO
    //= tracking-issue=449
    //= feature=AEAD limits
    //# Endpoints MUST count the number of encrypted packets for each set of
    //# keys.
    /// Return total number of packets encrypted with this key
    fn encrypted_packets(&self) -> usize;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::crypto::{
        retry::{IntegrityTag, INTEGRITY_TAG_LEN},
        CryptoError, HandshakeCrypto, HeaderCrypto, HeaderProtectionMask, InitialCrypto,
        OneRTTCrypto, RetryCrypto, ZeroRTTCrypto,
    };

    #[derive(Debug)]
    pub struct Key;

    impl super::Key for Key {
        /// Decrypt a payload
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
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

        fn encrypted_packets(&self) -> usize {
            todo!()
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
            Key
        }

        fn new_client(_connection_id: &[u8]) -> Self {
            Key
        }
    }
    impl HandshakeCrypto for Key {}
    impl OneRTTCrypto for Key {}
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
