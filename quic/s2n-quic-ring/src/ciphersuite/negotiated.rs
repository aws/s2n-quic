// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ciphersuite::{TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256},
    header_key::HeaderKey,
};
use core::fmt;
use ring::{aead, hkdf};
use s2n_quic_core::crypto::{self, CryptoError};

// ignore casing warnings in order to preserve the IANA name
#[allow(non_camel_case_types, clippy::all)]
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
    pub fn new(algorithm: &aead::Algorithm, secret: hkdf::Prk) -> Option<(Self, HeaderKey)> {
        Some(match algorithm {
            _ if algorithm == &aead::AES_256_GCM => {
                let (ciphersuite, header_key) = TLS_AES_256_GCM_SHA384::new(secret);
                (ciphersuite.into(), header_key)
            }
            _ if algorithm == &aead::CHACHA20_POLY1305 => {
                let (ciphersuite, header_key) = TLS_CHACHA20_POLY1305_SHA256::new(secret);
                (ciphersuite.into(), header_key)
            }
            _ if algorithm == &aead::AES_128_GCM => {
                let (ciphersuite, header_key) = TLS_AES_128_GCM_SHA256::new(secret);
                (ciphersuite.into(), header_key)
            }
            _ => return None,
        })
    }

    /// Update the ciphersuite as defined in
    /// https://www.rfc-editor.org/rfc/rfc9001.txt#6
    pub fn update(&self) -> Self {
        dispatch!(self, |cipher| cipher.update().into())
    }
}

impl crypto::Key for NegotiatedCiphersuite {
    #[inline]
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

    #[inline]
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

    #[inline]
    fn tag_len(&self) -> usize {
        dispatch!(self, |cipher| cipher.tag_len())
    }

    #[inline]
    fn aead_confidentiality_limit(&self) -> u64 {
        dispatch!(self, |cipher| cipher.aead_confidentiality_limit())
    }

    #[inline]
    fn aead_integrity_limit(&self) -> u64 {
        dispatch!(self, |cipher| cipher.aead_integrity_limit())
    }

    #[inline]
    fn ciphersuite(&self) -> s2n_quic_core::event::builder::Ciphersuite {
        dispatch!(self, |cipher| cipher.ciphersuite())
    }
}

impl fmt::Debug for NegotiatedCiphersuite {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        dispatch!(self, |cipher| cipher.fmt(f))
    }
}
