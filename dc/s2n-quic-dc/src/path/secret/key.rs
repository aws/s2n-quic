// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::map;
use crate::{
    credentials::Credentials,
    crypto::{awslc, decrypt, encrypt, IntoNonce, UninitSlice},
};
use core::mem::MaybeUninit;
use zeroize::Zeroize;

#[derive(Debug)]
pub struct Sealer {
    pub(super) sealer: awslc::EncryptKey,
}

impl encrypt::Key for Sealer {
    #[inline]
    fn credentials(&self) -> &Credentials {
        self.sealer.credentials()
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.sealer.tag_len()
    }

    #[inline]
    fn encrypt<N: IntoNonce>(
        &self,
        nonce: N,
        header: &[u8],
        extra_payload: Option<&[u8]>,
        payload_and_tag: &mut [u8],
    ) {
        self.sealer
            .encrypt(nonce, header, extra_payload, payload_and_tag)
    }

    #[inline]
    fn retransmission_tag(
        &self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        self.sealer.retransmission_tag(
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}

#[derive(Debug)]
pub struct Opener {
    pub(super) opener: awslc::DecryptKey,
    pub(super) dedup: map::Dedup,
}

impl Opener {
    /// Disables replay prevention allowing the decryption key to be reused.
    ///
    /// ## Safety
    /// Disabling replay prevention is insecure because it makes it possible for
    /// active network attackers to cause a peer to accept previously processed
    /// data as new. For example, if a packet contains a mutating request such
    /// as adding +1 to a value in a database, an attacker can keep replaying
    /// packets to increment the value beyond what the original legitimate
    /// sender of the packet intended.
    pub unsafe fn disable_replay_prevention(&mut self) {
        self.dedup.disable();
    }

    /// Ensures the key has not been used before
    #[inline]
    fn on_decrypt_success(&self, payload: &mut UninitSlice) -> decrypt::Result {
        self.dedup.check(&self.opener).map_err(|e| {
            let payload = unsafe {
                let ptr = payload.as_mut_ptr() as *mut MaybeUninit<u8>;
                let len = payload.len();
                core::slice::from_raw_parts_mut(ptr, len)
            };
            payload.zeroize();
            e
        })?;

        Ok(())
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn dedup_check(&self) -> decrypt::Result {
        self.dedup.check(&self.opener)
    }
}

impl decrypt::Key for Opener {
    #[inline]
    fn credentials(&self) -> &Credentials {
        self.opener.credentials()
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.opener.tag_len()
    }

    #[inline]
    fn decrypt<N: IntoNonce>(
        &self,
        nonce: N,
        header: &[u8],
        payload_in: &[u8],
        tag: &[u8],
        payload_out: &mut UninitSlice,
    ) -> decrypt::Result {
        self.opener
            .decrypt(nonce, header, payload_in, tag, payload_out)?;

        self.on_decrypt_success(payload_out)?;

        Ok(())
    }

    #[inline]
    fn decrypt_in_place<N: IntoNonce>(
        &self,
        nonce: N,
        header: &[u8],
        payload_and_tag: &mut [u8],
    ) -> decrypt::Result {
        self.opener
            .decrypt_in_place(nonce, header, payload_and_tag)?;

        self.on_decrypt_success(UninitSlice::new(payload_and_tag))?;

        Ok(())
    }

    #[inline]
    fn retransmission_tag(
        &self,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        self.opener.retransmission_tag(
            original_packet_number,
            retransmission_packet_number,
            tag_out,
        )
    }
}
