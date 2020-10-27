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
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::crypto::{
        CryptoError, HandshakeCrypto, HeaderCrypto, HeaderProtectionMask, InitialCrypto,
        OneRTTCrypto, ZeroRTTCrypto,
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
}
