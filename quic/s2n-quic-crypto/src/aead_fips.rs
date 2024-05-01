// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aead::{Aead, Result},
    ring_aead::{
        self, Aad, Nonce, TlsProtocolId, TlsRecordOpeningKey, TlsRecordSealingKey, MAX_TAG_LEN,
        NONCE_LEN,
    },
};
use s2n_quic_core::crypto::{packet_protection::Error, scatter};

/// Encryption keys backed by FIPS certified cryptography.
///
/// FipsKey is backed by [`TlsRecordSealingKey`], which enforces that nonces used with `seal_*`
/// operations are unique.
pub struct FipsKey {
    opener: TlsRecordOpeningKey,
    sealer: TlsRecordSealingKey,
}

impl FipsKey {
    #[inline]
    pub fn new(algorithm: &'static ring_aead::Algorithm, key_bytes: &[u8]) -> Result<Self> {
        let opener = TlsRecordOpeningKey::new(algorithm, TlsProtocolId::TLS13, key_bytes)
            .expect("key size verified");
        let sealer = TlsRecordSealingKey::new(algorithm, TlsProtocolId::TLS13, key_bytes)
            .expect("key size verified");
        Ok(FipsKey { opener, sealer })
    }
}

impl Aead for FipsKey {
    type Nonce = [u8; NONCE_LEN];
    type Tag = [u8; MAX_TAG_LEN];

    #[inline]
    fn encrypt(
        &mut self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result {
        use s2n_codec::Encoder;

        let nonce = Nonce::assume_unique_for_key(*nonce);
        let aad = Aad::from(aad);

        let buffer = payload.flatten();

        let tag = {
            let (input, _) = buffer.split_mut();

            self.sealer
                .seal_in_place_separate_tag(nonce, aad, input)
                .map_err(|_| Error::INTERNAL_ERROR)?
        };

        buffer.write_slice(tag.as_ref());

        Ok(())
    }

    #[inline]
    fn decrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        input: &mut [u8],
        tag: &[u8; MAX_TAG_LEN],
    ) -> Result {
        let nonce = Nonce::assume_unique_for_key(*nonce);
        let aad = Aad::from(aad);
        let input = unsafe {
            // ring requires that the input and tag be passed as a single slice
            // so we extend the input slice here.
            // This is only safe if they are contiguous
            debug_assert_eq!(
                if input.is_empty() {
                    (*input).as_ptr()
                } else {
                    (&input[input.len() - 1] as *const u8).add(1)
                },
                (*tag).as_ptr()
            );
            let ptr = input.as_mut_ptr();
            let len = input.len() + MAX_TAG_LEN;
            core::slice::from_raw_parts_mut(ptr, len)
        };
        self.opener
            .open_in_place(nonce, aad, input)
            .map_err(|_| Error::DECRYPT_ERROR)?;
        Ok(())
    }
}
