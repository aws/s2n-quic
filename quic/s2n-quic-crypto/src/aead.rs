// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::ring_aead::{Aad, LessSafeKey, Nonce, MAX_TAG_LEN, NONCE_LEN};
pub use s2n_quic_core::crypto::{packet_protection::Error, scatter};
pub type Result<T = (), E = Error> = core::result::Result<T, E>;

pub trait Aead {
    type Nonce;
    type Tag;

    fn encrypt(&self, nonce: &Self::Nonce, aad: &[u8], payload: &mut scatter::Buffer) -> Result;

    fn decrypt(
        &self,
        nonce: &Self::Nonce,
        aad: &[u8],
        payload: &mut [u8],
        tag: &Self::Tag,
    ) -> Result;
}

impl Aead for LessSafeKey {
    type Nonce = [u8; NONCE_LEN];
    type Tag = [u8; MAX_TAG_LEN];

    #[inline]
    #[cfg(target_os = "windows")]
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut scatter::Buffer,
    ) -> aead::Result {
        use s2n_codec::Encoder;

        let nonce = Nonce::assume_unique_for_key(*nonce);
        let aad = Aad::from(aad);

        let buffer = payload.flatten();

        let tag = {
            let (input, _) = buffer.split_mut();

            self.seal_in_place_separate_tag(nonce, aad, input)
                .map_err(|_| aead::Error::INTERNAL_ERROR)?
        };

        buffer.write_slice(tag.as_ref());

        Ok(())
    }

    // use the scatter API if we're using AWS-LC
    #[inline]
    #[cfg(not(target_os = "windows"))]
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result {
        let nonce = Nonce::assume_unique_for_key(*nonce);
        let aad = Aad::from(aad);

        let (buffer, extra) = payload.inner_mut();
        let extra_in = extra.as_deref().unwrap_or(&[][..]);
        let (in_out, extra_out_and_tag) = buffer.split_mut();
        let extra_out_and_tag = &mut extra_out_and_tag[..extra_in.len() + MAX_TAG_LEN];

        self.seal_in_place_scatter(nonce, aad, in_out, extra_in, extra_out_and_tag)
            .map_err(|_| Error::INTERNAL_ERROR)?;

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
        self.open_in_place(nonce, aad, input)
            .map_err(|_| Error::DECRYPT_ERROR)?;
        Ok(())
    }
}
