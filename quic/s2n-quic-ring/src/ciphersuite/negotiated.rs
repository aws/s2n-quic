use crate::ciphersuite::{
    TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256,
};
use core::fmt;
use ring::{aead, hkdf};
use s2n_quic_core::crypto::{CryptoError, HeaderCrypto, HeaderProtectionMask, Key};

#[allow(non_camel_case_types)]
pub enum NegotiatedCiphersuite {
    TLS_AES_256_GCM_SHA384(TLS_AES_256_GCM_SHA384),
    TLS_CHACHA20_POLY1305_SHA256(TLS_CHACHA20_POLY1305_SHA256),
    TLS_AES_128_GCM_SHA256(TLS_AES_128_GCM_SHA256),
}

/// Dispatch an operation to the negotiated ciphersuite
macro_rules! dispatch {
    ($self:ident, | $ciphersuite:ident | $expr:expr) => {
        match $self {
            Self::TLS_AES_256_GCM_SHA384($ciphersuite) => $expr,
            Self::TLS_CHACHA20_POLY1305_SHA256($ciphersuite) => $expr,
            Self::TLS_AES_128_GCM_SHA256($ciphersuite) => $expr,
        }
    };
}

impl From<TLS_AES_256_GCM_SHA384> for NegotiatedCiphersuite {
    fn from(ciphersuite: TLS_AES_256_GCM_SHA384) -> Self {
        Self::TLS_AES_256_GCM_SHA384(ciphersuite)
    }
}
impl From<TLS_CHACHA20_POLY1305_SHA256> for NegotiatedCiphersuite {
    fn from(ciphersuite: TLS_CHACHA20_POLY1305_SHA256) -> Self {
        Self::TLS_CHACHA20_POLY1305_SHA256(ciphersuite)
    }
}
impl From<TLS_AES_128_GCM_SHA256> for NegotiatedCiphersuite {
    fn from(ciphersuite: TLS_AES_128_GCM_SHA256) -> Self {
        Self::TLS_AES_128_GCM_SHA256(ciphersuite)
    }
}

impl NegotiatedCiphersuite {
    /// Create a ciphersuite with a given negotiated algorithm and secret
    pub fn new(algorithm: &aead::Algorithm, secret: hkdf::Prk) -> Option<Self> {
        Some(match algorithm {
            _ if algorithm == &aead::AES_256_GCM => TLS_AES_256_GCM_SHA384::new(secret).into(),
            _ if algorithm == &aead::CHACHA20_POLY1305 => {
                TLS_CHACHA20_POLY1305_SHA256::new(secret).into()
            }
            _ if algorithm == &aead::AES_128_GCM => TLS_AES_128_GCM_SHA256::new(secret).into(),
            _ => return None,
        })
    }

    /// Update the ciphersuite as defined in
    /// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
    pub fn update(&self) -> Self {
        dispatch!(self, |cipher| cipher.update().into())
    }
}

impl HeaderCrypto for NegotiatedCiphersuite {
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        dispatch!(self, |cipher| cipher.opening_header_protection_mask(sample))
    }

    fn opening_sample_len(&self) -> usize {
        dispatch!(self, |cipher| cipher.opening_sample_len())
    }

    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        dispatch!(self, |cipher| cipher.sealing_header_protection_mask(sample))
    }

    fn sealing_sample_len(&self) -> usize {
        dispatch!(self, |cipher| cipher.sealing_sample_len())
    }
}

impl Key for NegotiatedCiphersuite {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        dispatch!(self, |cipher| cipher.decrypt(
            packet_number,
            header,
            payload
        ))
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        dispatch!(self, |cipher| cipher.encrypt(
            packet_number,
            header,
            payload
        ))
    }

    fn tag_len(&self) -> usize {
        dispatch!(self, |cipher| cipher.tag_len())
    }

    fn aead_confidentiality_limit(&self) -> usize {
        dispatch!(self, |cipher| cipher.aead_confidentiality_limit())
    }

    fn aead_integrity_limit(&self) -> usize {
        dispatch!(self, |cipher| cipher.aead_integrity_limit())
    }
}

impl fmt::Debug for NegotiatedCiphersuite {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        dispatch!(self, |cipher| cipher.fmt(f))
    }
}
