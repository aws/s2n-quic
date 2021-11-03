// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aes::Encrypt,
    aesgcm::{
        payload::{DecryptionPayload, Payload},
        Error, NONCE_LEN, TAG_LEN,
    },
    block::{Batch, BatchMut, Block, Zeroed, LEN as BLOCK_LEN},
    ctr::Ctr,
    ghash::GHash,
};
use core::{
    marker::PhantomData,
    sync::atomic::{compiler_fence, Ordering},
};
use subtle::ConstantTimeEq;

pub struct AesGcm<Aes, GHash, Ctr, const N: usize> {
    aes: Aes,
    ghash: GHash,
    ctr: PhantomData<Ctr>,
}

impl<A, G, C, const N: usize> AesGcm<A, G, C, N>
where
    C: Ctr,
    G: GHash,
{
    #[inline(always)]
    pub fn new(aes: A, ghash: G) -> Self {
        Self {
            aes,
            ghash,
            ctr: PhantomData,
        }
    }
}

impl<A, G, C, B, const N: usize> AesGcm<A, G, C, N>
where
    A: Encrypt<Block = B>,
    G: GHash<Block = B>,
    C: Ctr<Block = B>,
    B: Block + Batch<Block = B> + BatchMut,
    [B; N]: Batch<Block = B> + BatchMut + Zeroed,
{
    #[inline(always)]
    fn aesgcm<P: Payload<B>>(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        mut payload: P,
    ) -> [u8; TAG_LEN] {
        // ask how many blocks are in the AAD
        let aad_len = aad.len();
        // payload includes the tag so don't round up
        let payload_len = payload.len();

        // create a block with the AAD and payload lengths
        let bit_counts = C::bit_counts(aad_len, payload_len);

        // keep track of the number of blocks required to hash
        // start off with 1 for the `bit_counts` block
        let mut required_blocks = 1;

        // load the AAD into the first ghash batch
        let (mut ghash_blocks, mut aad_block_count) = aad_blocks(aad);

        // add the AAD blocks to the required size
        required_blocks += aad_block_count;

        // initialize the cipher blocks to hold the encryption stream
        let mut cipher_blocks = [B::zeroed(); N];

        // compute the number of blocks the payload will require
        let mut payload_block_count = payload_len / BLOCK_LEN;
        let batch_rem = payload_block_count % N;
        let mut partial_blocks = batch_rem;
        let payload_rem = payload_len % BLOCK_LEN;

        // set to true if the ek0 counter can be encrypted alongside the last batch
        let can_interleave_ek0;
        // set it to an out of bounds index if we don't need it
        let mut last_block_idx = N + 1;

        if payload_rem == 0 {
            can_interleave_ek0 = true;
        } else {
            // add another partial block to be processed
            required_blocks += 1;
            partial_blocks += 1;
            // set the last block index to the batch remainder
            last_block_idx = batch_rem;
            // we can only interleave ek0 if we have a spare block
            can_interleave_ek0 = last_block_idx < N - 1;
        }

        required_blocks += payload_block_count;

        // initialize the ghash state with the number of blocks we plan on hashing
        let mut ghash_state = self.ghash.start(required_blocks);

        // initialize the counter with the provided nonce
        let mut ctr = C::new(nonce);

        // generate the ek0 counter block to be encrypted and hashed at the end
        let mut ek0 = ctr.block();

        /// Performs a single batch which will interleave the AES-CTR stream
        /// cipher with the previous GHash batch
        macro_rules! batch {
            ($ghash_check:expr, $ghash_after:expr) => {
                // initialize the cipher blocks with the current counter values
                cipher_blocks.update(
                    #[inline(always)]
                    |_idx, block| {
                        ctr.increment();
                        *block = ctr.block();
                    },
                );

                // if this is the last batch and we have a partial batch
                // then fill the last index with the ek0 block
                if can_interleave_ek0 && payload_block_count < N {
                    unsafe {
                        unsafe_assert!(cipher_blocks.len() > N - 1);
                        *cipher_blocks.get_unchecked_mut(N - 1) = ek0;
                    }
                }

                // encrypt the cipher blocks and interleave ghash
                self.aes.encrypt_interleaved(
                    &mut cipher_blocks,
                    #[inline(always)]
                    |idx| {
                        if idx >= N || !($ghash_check) {
                            return;
                        }

                        let block = unsafe {
                            unsafe_assert!(idx < ghash_blocks.len());
                            ghash_blocks.get_unchecked(idx)
                        };
                        self.ghash.update(&mut ghash_state, block);
                        $ghash_after;

                        // force the compiler to interleave the AES and GHash instructions.
                        // without this, it will reorder and be drastically slower
                        compiler_fence(Ordering::SeqCst);
                    },
                );
            };
        }

        // perform an initial batch for the AAD and initial payload
        batch!(aad_block_count > 0, aad_block_count -= 1);

        // iterate over all of the remaining full batches
        while let Some(count) = payload_block_count.checked_sub(N) {
            payload_block_count = count;

            // Apply the AES-CTR stream cipher blocks to the payload and move them into
            // the ghash blocks
            cipher_blocks.for_each(
                #[inline(always)]
                |idx, block| {
                    // XOR the cipher blocks into the payload
                    let ghash_block = unsafe {
                        unsafe_assert!(payload.len() >= BLOCK_LEN);
                        let payload_block = payload.read_block();
                        payload.xor_block(payload_block, *block)
                    };

                    // move the cipher blocks to be hashed on the next batch
                    unsafe {
                        unsafe_assert!(idx < ghash_blocks.len());
                        *ghash_blocks.get_unchecked_mut(idx) = ghash_block;
                    }
                },
            );

            // apply a full batch without any constraints
            batch!(true, {});
        }

        unsafe {
            unsafe_assert!(
                partial_blocks <= N,
                "only a single batch should be left to process"
            );
        }

        // finalize the encryption stream
        if partial_blocks > 0 {
            cipher_blocks.for_each(
                #[inline(always)]
                |idx, cipher_block| {
                    if idx >= partial_blocks {
                        return;
                    }

                    // XOR the cipher blocks into the payload
                    let ghash_block = if idx == last_block_idx {
                        unsafe {
                            unsafe_assert!(0 < payload.len() && payload.len() < BLOCK_LEN);
                            let payload_block = payload.read_last_block(payload_rem);
                            payload.xor_last_block(payload_block, *cipher_block, payload_rem)
                        }
                    } else {
                        unsafe {
                            unsafe_assert!(payload.len() >= BLOCK_LEN);
                            let payload_block = payload.read_block();
                            payload.xor_block(payload_block, *cipher_block)
                        }
                    };

                    self.ghash.update(&mut ghash_state, &ghash_block);
                },
            );
        }

        // if we had spare capacity then extract the ek0 value, otherwise it needs
        // to be encrypted in its own round
        if can_interleave_ek0 {
            ek0 = unsafe {
                unsafe_assert!(cipher_blocks.len() > N - 1);
                *cipher_blocks.get_unchecked(N - 1)
            };
        } else {
            self.aes.encrypt(&mut ek0);
        }

        // hash the aad and payload bit counts
        self.ghash.update(&mut ghash_state, &bit_counts);

        // finalize the ghash and xor the tag with the encrypted ek0
        self.ghash.finish(ghash_state).xor(ek0).into_array()
    }
}

impl<A, G, C, B, const N: usize> crate::aesgcm::AesGcm for AesGcm<A, G, C, N>
where
    A: Encrypt<Block = B>,
    G: GHash<Block = B>,
    C: Ctr<Block = B>,
    B: Block + Batch<Block = B> + BatchMut,
    [B; N]: Batch<Block = B> + BatchMut + Zeroed,
    for<'a> &'a mut [u8]: Payload<B>,
{
    #[inline(always)]
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut [u8],
        tag: &mut [u8; TAG_LEN],
    ) {
        *tag = Self::aesgcm(self, nonce, aad, payload)
    }

    #[inline(always)]
    fn decrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<(), super::Error> {
        // wrap the payload in one that returns the payload block instead of the XOR'd
        let payload = DecryptionPayload(payload);

        let expected_tag = Self::aesgcm(self, nonce, aad, payload);

        // we don't want the compiler to perform any tag checks until the very end
        compiler_fence(Ordering::SeqCst);

        if tag.ct_eq(&expected_tag).into() {
            Ok(())
        } else {
            Err(Error)
        }
    }
}

#[inline(always)]
fn aad_blocks<B, const N: usize>(aad: &[u8]) -> ([B; N], usize)
where
    B: Block,
    [B; N]: Zeroed,
{
    let len = aad.len();

    let block_count = (aad.len() + BLOCK_LEN - 1) / BLOCK_LEN;

    let mut blocks = <[B; N]>::zeroed();

    unsafe {
        // since QUIC short packets only contain small AAD values, we can limit the
        // amount of work to a single batch size.
        unsafe_assert!(
            len <= N * BLOCK_LEN,
            "aad cannot exceed {} bytes; got {}",
            N * BLOCK_LEN,
            len,
        );

        // copy the AAD slice into a batch array
        core::ptr::copy_nonoverlapping(aad.as_ptr(), blocks.as_mut_ptr() as *mut u8, len);
    }

    (blocks, block_count)
}
