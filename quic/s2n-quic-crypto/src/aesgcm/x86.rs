// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aesgcm::payload::Payload,
    arch::*,
    block::{
        x86::{M128iExt, LEN as BLOCK_LEN},
        Block,
    },
};
use s2n_quic_core::assume;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

impl Payload<__m128i> for &mut [u8] {
    #[inline(always)]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline(always)]
    unsafe fn read_block(&self) -> __m128i {
        assume!(self.len() >= BLOCK_LEN);
        _mm_loadu_si128(*self as *const _ as *const _)
    }

    #[inline(always)]
    unsafe fn xor_block(&mut self, cleartext_block: __m128i, aes_block: __m128i) -> __m128i {
        assume!(self.len() >= BLOCK_LEN);
        let addr = *self as *mut [u8] as *mut u8;

        // read the cleartext block and XOR it with the provided AES block
        let xored = cleartext_block.xor(aes_block);

        // write the XOR'd block back to the slice
        _mm_storeu_si128(addr as *mut __m128i, xored);

        // move the slice forward by a block
        let addr = addr.add(BLOCK_LEN);
        let new_len = self.len() - BLOCK_LEN;
        *self = core::slice::from_raw_parts_mut(addr, new_len);

        xored
    }

    #[inline(always)]
    unsafe fn read_last_block(&self, len: usize) -> __m128i {
        assume!(0 < len && len < BLOCK_LEN);
        assume!(self.len() == len);
        __m128i::from_slice(self)
    }

    #[inline(always)]
    unsafe fn xor_last_block(
        &mut self,
        cleartext_block: __m128i,
        aes_block: __m128i,
        len: usize,
    ) -> __m128i {
        assume!(0 < len && len < BLOCK_LEN);
        assume!(self.len() == len);
        let addr = *self as *mut [u8] as *mut u8;

        let xored = cleartext_block.xor(aes_block.mask(len));

        // write the XOR'd block back to the slice
        xored.into_slice(self);

        // make the slice empty
        *self = core::slice::from_raw_parts_mut(addr, 0);

        xored
    }
}
