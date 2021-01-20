use crate::ciphersuite::TLS_AES_128_GCM_SHA256 as Ciphersuite;
use s2n_quic_core::crypto::{CryptoError, HeaderCrypto, HeaderProtectionMask, Key, ZeroRTTCrypto};

#[derive(Debug)]
pub struct RingZeroRTTCrypto(Ciphersuite);

impl RingZeroRTTCrypto {
    /// Create a ZeroRTT ciphersuite with a given secret
    pub fn new(secret: crate::Prk) -> Self {
        Self(Ciphersuite::new(secret))
    }
}

impl ZeroRTTCrypto for RingZeroRTTCrypto {}

impl Key for RingZeroRTTCrypto {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.0.decrypt(packet_number, header, payload)
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.0.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> usize {
        self.0.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> usize {
        self.0.aead_integrity_limit()
    }
}

impl HeaderCrypto for RingZeroRTTCrypto {
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.0.opening_header_protection_mask(sample)
    }

    fn opening_sample_len(&self) -> usize {
        self.0.opening_sample_len()
    }

    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.0.sealing_header_protection_mask(sample)
    }

    fn sealing_sample_len(&self) -> usize {
        self.0.sealing_sample_len()
    }
}
