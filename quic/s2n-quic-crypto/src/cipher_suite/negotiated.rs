// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aws_lc_aead as aead,
    cipher_suite::{TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256},
    header_key::HeaderKey,
    hkdf,
};
use core::fmt;
use s2n_quic_core::crypto::{self, packet_protection, scatter};

// ignore casing warnings in order to preserve the IANA name
#[allow(non_camel_case_types, clippy::all)]
pub enum NegotiatedCipherSuite {
    TLS_AES_256_GCM_SHA384(TLS_AES_256_GCM_SHA384),
    TLS_CHACHA20_POLY1305_SHA256(TLS_CHACHA20_POLY1305_SHA256),
    TLS_AES_128_GCM_SHA256(TLS_AES_128_GCM_SHA256),
}

/// Dispatch an operation to the negotiated cipher_suite
macro_rules! dispatch {
    ($self:ident, | $cipher_suite:ident | $expr:expr) => {
        match $self {
            Self::TLS_AES_256_GCM_SHA384($cipher_suite) => $expr,
            Self::TLS_CHACHA20_POLY1305_SHA256($cipher_suite) => $expr,
            Self::TLS_AES_128_GCM_SHA256($cipher_suite) => $expr,
        }
    };
}

impl From<TLS_AES_256_GCM_SHA384> for NegotiatedCipherSuite {
    fn from(cipher_suite: TLS_AES_256_GCM_SHA384) -> Self {
        Self::TLS_AES_256_GCM_SHA384(cipher_suite)
    }
}

impl From<TLS_CHACHA20_POLY1305_SHA256> for NegotiatedCipherSuite {
    fn from(cipher_suite: TLS_CHACHA20_POLY1305_SHA256) -> Self {
        Self::TLS_CHACHA20_POLY1305_SHA256(cipher_suite)
    }
}

impl From<TLS_AES_128_GCM_SHA256> for NegotiatedCipherSuite {
    fn from(cipher_suite: TLS_AES_128_GCM_SHA256) -> Self {
        Self::TLS_AES_128_GCM_SHA256(cipher_suite)
    }
}

impl NegotiatedCipherSuite {
    /// Create a cipher_suite with a given negotiated algorithm and secret
    pub fn new(algorithm: &aead::Algorithm, secret: hkdf::Prk) -> Option<(Self, HeaderKey)> {
        Some(match algorithm {
            _ if algorithm == &aead::AES_256_GCM => {
                let (cipher_suite, header_key) = TLS_AES_256_GCM_SHA384::new(secret);
                (cipher_suite.into(), header_key)
            }
            _ if algorithm == &aead::CHACHA20_POLY1305 => {
                let (cipher_suite, header_key) = TLS_CHACHA20_POLY1305_SHA256::new(secret);
                (cipher_suite.into(), header_key)
            }
            _ if algorithm == &aead::AES_128_GCM => {
                let (cipher_suite, header_key) = TLS_AES_128_GCM_SHA256::new(secret);
                (cipher_suite.into(), header_key)
            }
            _ => return None,
        })
    }

    /// Update the cipher_suite as defined in
    /// https://www.rfc-editor.org/rfc/rfc9001#section-6
    pub fn update(&self) -> Self {
        dispatch!(self, |cipher| cipher.update().into())
    }
}

impl crypto::Key for NegotiatedCipherSuite {
    #[inline]
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), packet_protection::Error> {
        dispatch!(self, |cipher| cipher.decrypt(
            packet_number,
            header,
            payload
        ))
    }

    #[inline]
    fn encrypt(
        &mut self,
        packet_number: u64,
        header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), packet_protection::Error> {
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
    fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
        dispatch!(self, |cipher| cipher.cipher_suite())
    }
}

impl fmt::Debug for NegotiatedCipherSuite {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        dispatch!(self, |cipher| cipher.fmt(f))
    }
}
