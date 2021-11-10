// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aesgcm::NONCE_LEN,
    arch::*,
    block::{x86::M128iExt, Block},
    ctr,
};

#[derive(Clone, Copy, Debug)]
pub struct Ctr(__m128i);

impl ctr::Ctr for Ctr {
    type Block = __m128i;

    #[inline(always)]
    fn new(nonce: &[u8; NONCE_LEN]) -> Self {
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/gcm.c#L249
        //
        // OPENSSL_memcpy(ctx->Yi.c, iv, 12);
        // ctx->Yi.c[15] = 1;
        let mut ctr = [0u8; 16];
        ctr[..12].copy_from_slice(nonce);
        ctr[15] = 1;
        let ctr = __m128i::from_array(ctr).reverse();
        Self(ctr)
    }

    #[inline(always)]
    fn block(&self) -> __m128i {
        self.0.reverse()
    }

    #[inline(always)]
    fn increment(&mut self) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            let one = _mm_set_epi64x(0, 1);
            self.0 = _mm_add_epi64(self.0, one);
        }
    }
}
