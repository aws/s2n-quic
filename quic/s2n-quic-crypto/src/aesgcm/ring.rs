// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aead::{self, scatter},
    aesgcm::{NONCE_LEN, TAG_LEN},
    ring_aead::{Aad, LessSafeKey, Nonce},
};

impl aead::Aead for LessSafeKey {
    type Nonce = [u8; NONCE_LEN];
    type Tag = [u8; TAG_LEN];

    #[inline]
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

    /*
     * TODO: enable this once the scatter API is available in AWS-LC-RS
     *
     * https://github.com/aws/aws-lc-rs/pull/206
     *
    #[inline]
    #[cfg(target_os = "linux")]
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut scatter::Buffer,
    ) -> aead::Result {
        let nonce = Nonce::assume_unique_for_key(*nonce);
        let aad = Aad::from(aad);

        let (buffer, extra) = payload.inner_mut();
        let extra_in = extra.as_deref().unwrap_or(&[][..]);
        let (in_out, extra_out_and_tag) = buffer.split_mut();
        let extra_out_and_tag = &mut extra_out_and_tag[..extra_in.len() + TAG_LEN];

        self.seal_in_place_scatter(nonce, aad, in_out, extra_in, extra_out_and_tag)
            .map_err(|_| aead::Error::INTERNAL_ERROR)?;

        Ok(())
    }
    */

    #[inline]
    fn decrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        input: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> aead::Result {
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
            let len = input.len() + TAG_LEN;
            core::slice::from_raw_parts_mut(ptr, len)
        };
        self.open_in_place(nonce, aad, input)
            .map_err(|_| aead::Error::DECRYPT_ERROR)?;
        Ok(())
    }
}

macro_rules! impl_aesgcm {
    ($name:ident, $lower:ident) => {
        #[cfg(any(test, feature = "testing"))]
        pub mod $lower {
            use crate::{
                aesgcm::testing::$lower::Implementation,
                ring_aead::{$name, LessSafeKey, UnboundKey},
            };

            pub fn implementations(impls: &mut Vec<Implementation>) {
                impls.push(Implementation {
                    name: "ring",
                    new: |key| {
                        let key = UnboundKey::new(&$name, &key).unwrap();
                        let key = LessSafeKey::new(key);
                        Box::new(key)
                    },
                });
            }
        }
    };
}

impl_aesgcm!(AES_128_GCM, aes128);
impl_aesgcm!(AES_256_GCM, aes256);
