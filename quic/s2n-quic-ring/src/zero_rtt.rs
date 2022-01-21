// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{ciphersuite::TLS_AES_128_GCM_SHA256 as Ciphersuite, header_key::HeaderKey};
use s2n_quic_core::crypto::{
    self, CryptoError, HeaderProtectionMask, Key, ZeroRttHeaderKey, ZeroRttKey,
};

#[derive(Debug)]
pub struct RingZeroRttKey(Ciphersuite);

impl RingZeroRttKey {
    /// Create a ZeroRTT ciphersuite with a given secret
    pub fn new(secret: crate::Prk) -> (Self, RingZeroRttHeaderKey) {
        let (key, header_key) = Ciphersuite::new(secret);
        let key = Self(key);
        let header_key = RingZeroRttHeaderKey(header_key);
        (key, header_key)
    }
}

impl ZeroRttKey for RingZeroRttKey {}

impl Key for RingZeroRttKey {
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

    fn aead_confidentiality_limit(&self) -> u64 {
        self.0.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.0.aead_integrity_limit()
    }

    #[inline]
    fn ciphersuite(&self) -> s2n_quic_core::event::builder::Ciphersuite {
        self.0.ciphersuite()
    }
}

#[derive(Debug)]
pub struct RingZeroRttHeaderKey(HeaderKey);

impl crypto::HeaderKey for RingZeroRttHeaderKey {
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

impl ZeroRttHeaderKey for RingZeroRttHeaderKey {}
