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
