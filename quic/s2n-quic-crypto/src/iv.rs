// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::hkdf;
use s2n_codec::{Encoder, EncoderBuffer};
use zeroize::Zeroize;

pub use crate::ring_aead::NONCE_LEN;

pub struct Iv([u8; NONCE_LEN]);

impl Iv {
    #[inline]
    pub fn new(secret: &hkdf::Prk, label: &[u8]) -> Self {
        let mut bytes = [0u8; NONCE_LEN];

        secret
            .expand(&[label], IvLen)
            .expect("label size verified")
            .fill(&mut bytes)
            .expect("fill size verified");

        Self(bytes)
    }

    #[inline]
    pub fn nonce(&self, packet_number: u64) -> [u8; NONCE_LEN] {
        let mut nonce = [0; NONCE_LEN];
        let mut encoder = EncoderBuffer::new(&mut nonce);

        encoder.encode(&0u32);
        encoder.encode(&packet_number);

        for (a, b) in nonce.iter_mut().zip(self.0.iter()) {
            *a ^= b;
        }

        nonce
    }
}

impl Zeroize for Iv {
    #[inline]
    fn zeroize(&mut self) {
        // deref to a slice to we can take advantage of the bulk zeroization
        self.0.zeroize()
    }
}

struct IvLen;

impl hkdf::KeyType for IvLen {
    #[inline]
    fn len(&self) -> usize {
        NONCE_LEN
    }
}
