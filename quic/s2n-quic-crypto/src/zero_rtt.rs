// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{cipher_suite::TLS_AES_128_GCM_SHA256 as CipherSuite, header_key::HeaderKey};
use s2n_quic_core::crypto::{self, scatter, CryptoError, HeaderProtectionMask, Key};

#[derive(Debug)]
pub struct ZeroRttKey(CipherSuite);

impl ZeroRttKey {
    /// Create a ZeroRTT cipher suite with a given secret
    pub fn new(secret: crate::Prk) -> (Self, ZeroRttHeaderKey) {
        let (key, header_key) = CipherSuite::new(secret);
        let key = Self(key);
        let header_key = ZeroRttHeaderKey(header_key);
        (key, header_key)
    }
}

impl crypto::ZeroRttKey for ZeroRttKey {}

impl Key for ZeroRttKey {
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
        payload: &mut scatter::Buffer,
    ) -> Result<(), CryptoError> {
        self.0.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.0.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.0.aead_integrity_limit()
    }

    #[inline]
    fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
        self.0.cipher_suite()
    }
}

#[derive(Debug)]
pub struct ZeroRttHeaderKey(HeaderKey);

impl crypto::HeaderKey for ZeroRttHeaderKey {
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

impl crypto::ZeroRttHeaderKey for ZeroRttHeaderKey {}
