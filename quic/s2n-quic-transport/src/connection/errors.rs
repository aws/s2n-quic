//! Defines error types for packet handling inside connections

use s2n_quic_core::crypto::CryptoError;

/// Errors that can occur during packet reception
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PacketHandlingError {
    /// A crypto error occured
    CryptoError(CryptoError),
}

impl From<CryptoError> for PacketHandlingError {
    fn from(error: CryptoError) -> Self {
        Self::CryptoError(error)
    }
}
