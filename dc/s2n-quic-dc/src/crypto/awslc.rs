// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::IntoNonce;
use crate::credentials::Credentials;
use aws_lc_rs::aead::{Aad, Algorithm, LessSafeKey, Nonce, UnboundKey, NONCE_LEN};
use s2n_quic_core::assume;

pub use aws_lc_rs::aead::{AES_128_GCM, AES_256_GCM};

const TAG_LEN: usize = 16;

#[derive(Debug)]
pub struct EncryptKey {
    credentials: Credentials,
    key: LessSafeKey,
    iv: Iv,
}

impl EncryptKey {
    #[inline]
    pub fn new(
        credentials: Credentials,
        key: &[u8],
        iv: [u8; NONCE_LEN],
        algorithm: &'static Algorithm,
    ) -> Self {
        let key = UnboundKey::new(algorithm, key).unwrap();
        let key = LessSafeKey::new(key);
        Self {
            credentials,
            key,
            iv: Iv(iv),
        }
    }
}

impl super::encrypt::Key for &EncryptKey {
    #[inline]
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline(always)]
    fn tag_len(&self) -> usize {
        debug_assert_eq!(TAG_LEN, self.key.algorithm().tag_len());
        TAG_LEN
    }

    #[inline]
    fn encrypt<N: IntoNonce>(
        &self,
        nonce: N,
        header: &[u8],
        extra_payload: Option<&[u8]>,
        payload_and_tag: &mut [u8],
    ) {
        let nonce = self.iv.nonce(nonce);
        let aad = Aad::from(header);

        let extra_in = extra_payload.unwrap_or(&[][..]);

        unsafe {
            assume!(payload_and_tag.len() >= self.tag_len() + extra_in.len());
        }

        let inline_len = payload_and_tag.len() - self.tag_len() - extra_in.len();

        unsafe {
            assume!(payload_and_tag.len() >= inline_len);
        }
        let (in_out, extra_out_and_tag) = payload_and_tag.split_at_mut(inline_len);

        let result =
            self.key
                .seal_in_place_scatter(nonce, aad, in_out, extra_in, extra_out_and_tag);

        unsafe {
            assume!(result.is_ok());
        }
    }

    #[inline]
    fn retransmission_tag(
        &self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        retransmission_tag(
            &self.key,
            &self.iv,
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}

impl super::encrypt::Key for EncryptKey {
    #[inline]
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    fn tag_len(&self) -> usize {
        <&Self as super::encrypt::Key>::tag_len(&self)
    }

    #[inline]
    fn encrypt<N: IntoNonce>(
        &self,
        nonce: N,
        header: &[u8],
        extra_payload: Option<&[u8]>,
        payload_and_tag: &mut [u8],
    ) {
        <&Self as super::encrypt::Key>::encrypt(
            &self,
            nonce,
            header,
            extra_payload,
            payload_and_tag,
        )
    }

    #[inline]
    fn retransmission_tag(
        &self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        <&Self as super::encrypt::Key>::retransmission_tag(
            &self,
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}

#[derive(Debug)]
pub struct DecryptKey {
    credentials: Credentials,
    key: LessSafeKey,
    iv: Iv,
}

impl DecryptKey {
    #[inline]
    pub fn new(
        credentials: Credentials,
        key: &[u8],
        iv: [u8; NONCE_LEN],
        algorithm: &'static Algorithm,
    ) -> Self {
        let key = UnboundKey::new(algorithm, key).unwrap();
        let key = LessSafeKey::new(key);
        Self {
            credentials,
            key,
            iv: Iv(iv),
        }
    }
}

impl super::decrypt::Key for &DecryptKey {
    #[inline]
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    fn tag_len(&self) -> usize {
        debug_assert_eq!(TAG_LEN, self.key.algorithm().tag_len());
        TAG_LEN
    }

    #[inline]
    fn decrypt<N: IntoNonce>(
        &mut self,
        nonce: N,
        header: &[u8],
        payload_in: &[u8],
        tag: &[u8],
        payload_out: &mut super::UninitSlice,
    ) -> super::decrypt::Result {
        debug_assert_eq!(payload_in.len(), payload_out.len());

        let nonce = self.iv.nonce(nonce);
        let aad = Aad::from(header);

        let payload_out = unsafe {
            // SAFETY: the payload is not read by aws-lc, only written to
            let ptr = payload_out.as_mut_ptr();
            let len = payload_out.len();
            core::slice::from_raw_parts_mut(ptr, len)
        };

        self.key
            .open_separate_gather(nonce, aad, payload_in, tag, payload_out)
            .map_err(|_| super::decrypt::Error::InvalidTag)
    }

    #[inline]
    fn decrypt_in_place<N: IntoNonce>(
        &mut self,
        nonce: N,
        header: &[u8],
        payload_and_tag: &mut [u8],
    ) -> super::decrypt::Result {
        let nonce = self.iv.nonce(nonce);
        let aad = Aad::from(header);

        self.key
            .open_in_place(nonce, aad, payload_and_tag)
            .map_err(|_| super::decrypt::Error::InvalidTag)?;

        Ok(())
    }

    #[inline]
    fn retransmission_tag(
        &mut self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        retransmission_tag(
            &self.key,
            &self.iv,
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}

impl super::decrypt::Key for DecryptKey {
    fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    #[inline]
    fn tag_len(&self) -> usize {
        <&Self as super::decrypt::Key>::tag_len(&self)
    }

    #[inline]
    fn decrypt<N: IntoNonce>(
        &mut self,
        nonce: N,
        header: &[u8],
        payload_in: &[u8],
        tag: &[u8],
        payload_out: &mut bytes::buf::UninitSlice,
    ) -> super::decrypt::Result {
        <&Self as super::decrypt::Key>::decrypt(
            &mut &*self,
            nonce,
            header,
            payload_in,
            tag,
            payload_out,
        )
    }

    #[inline]
    fn decrypt_in_place<N: IntoNonce>(
        &mut self,
        nonce: N,
        header: &[u8],
        payload_and_tag: &mut [u8],
    ) -> super::decrypt::Result {
        <&Self as super::decrypt::Key>::decrypt_in_place(
            &mut &*self,
            nonce,
            header,
            payload_and_tag,
        )
    }

    #[inline]
    fn retransmission_tag(
        &mut self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        <&Self as super::decrypt::Key>::retransmission_tag(
            &mut &*self,
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}

#[inline]
fn retransmission_tag(
    key: &LessSafeKey,
    iv: &Iv,
    original_packet_number: u64,
    retransmission_packet_number: u64,
    tag_out: &mut [u8],
) {
    debug_assert_eq!(tag_out.len(), TAG_LEN);

    let nonce = iv.nonce(retransmission_packet_number);
    let aad = original_packet_number.to_be_bytes();
    let aad = Aad::from(&aad);

    let tag = key.seal_in_place_separate_tag(nonce, aad, &mut []).unwrap();

    for (a, b) in tag_out.iter_mut().zip(tag.as_ref()) {
        *a ^= b;
    }
}

#[derive(Debug)]
struct Iv([u8; NONCE_LEN]);

impl Iv {
    #[inline]
    fn nonce<N: IntoNonce>(&self, nonce: N) -> Nonce {
        let mut nonce = nonce.into_nonce();
        for (dst, src) in nonce.iter_mut().zip(&self.0) {
            *dst ^= src;
        }
        Nonce::assume_unique_for_key(nonce)
    }
}
