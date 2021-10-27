// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub trait Payload<Block: Copy> {
    fn len(&self) -> usize;
    unsafe fn read_block(&self) -> Block;
    unsafe fn xor_block(&mut self, payload_block: Block, aes_block: Block) -> Block;
    unsafe fn read_last_block(&self, len: usize) -> Block;
    unsafe fn xor_last_block(
        &mut self,
        payload_block: Block,
        aes_block: Block,
        len: usize,
    ) -> Block;
}

/// A payload wrapper that returns the payload blocks, rather than the XOR'd block
///
/// This is used when decrypting payloads, as GHash expects the encrypted bytes, rather
/// than the cleartext.
pub struct DecryptionPayload<P>(pub P);

impl<P, Block> Payload<Block> for DecryptionPayload<P>
where
    P: Payload<Block>,
    Block: Copy,
{
    #[inline(always)]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    unsafe fn read_block(&self) -> Block {
        self.0.read_block()
    }

    #[inline(always)]
    unsafe fn xor_block(&mut self, payload_block: Block, aes_block: Block) -> Block {
        self.0.xor_block(payload_block, aes_block);
        payload_block
    }

    #[inline(always)]
    unsafe fn read_last_block(&self, len: usize) -> Block {
        self.0.read_last_block(len)
    }

    #[inline(always)]
    unsafe fn xor_last_block(
        &mut self,
        payload_block: Block,
        aes_block: Block,
        len: usize,
    ) -> Block {
        self.0.xor_last_block(payload_block, aes_block, len);
        payload_block
    }
}
