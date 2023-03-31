// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::block::BatchMut;
use zeroize::Zeroize;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub trait Encrypt {
    type Block;
    const KEY_LEN: usize;
    const ROUNDS: usize;

    /// Encrypts a batch of blocks with the AES key
    #[inline(always)]
    fn encrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
        self.encrypt_interleaved(block, |_| {})
    }

    /// Encrypts a batch of blocks with the AES key, while interleaving the provided function
    ///
    /// The provided function will be called for each AES round of the key. This can lead to
    /// improved performance if multiple operations are being done in parallel.
    fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
        &self,
        block: &mut B,
        f: F,
    );
}

pub trait Decrypt {
    type Block;
    const KEY_LEN: usize;
    const ROUNDS: usize;

    /// Decrypts a batch of blocks with the AES key
    #[inline(always)]
    fn decrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
        self.decrypt_interleaved(block, |_| {})
    }

    /// Decrypts a batch of blocks with the AES key, while interleaving the provided function
    ///
    /// The provided function will be called for each AES round of the key. This can lead to
    /// improved performance if multiple operations are being done in parallel.
    fn decrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
        &self,
        block: &mut B,
        f: F,
    );
}

pub trait KeyRound {
    type Block;

    fn xor<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
    fn encrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
    fn encrypt_finish<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
    fn decrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
    fn decrypt_finish<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
}

#[cfg(any(test, feature = "testing"))]
pub const BLOCK_LEN: usize = 16;

pub mod aes128 {
    use super::*;

    pub const KEY_LEN: usize = 16;
    // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4378
    // mov	\$9,$bits			# 10 rounds for 128-bit key
    pub const ROUNDS: usize = 10;

    #[derive(Zeroize)]
    pub struct Key<T>(pub T);

    impl<Blk, T> Encrypt for Key<T>
    where
        T: EncryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;
        const ROUNDS: usize = ROUNDS;

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Blk>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            f: F,
        ) {
            aes128::EncryptionKey::encrypt_interleaved(&self.0, block, f)
        }
    }

    pub trait EncryptionKey {
        type Block;
        type KeyRound: KeyRound<Block = Self::Block>;

        fn keyround(&self, index: usize) -> &Self::KeyRound;

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            // NOTE: instead of looping here, manually unroll the loop so the CPU has a large run of instructions
            //       without any branches.
            self.keyround(0).xor(block);
            self.keyround(1).encrypt(block);
            f(0);
            self.keyround(2).encrypt(block);
            f(1);
            self.keyround(3).encrypt(block);
            f(2);
            self.keyround(4).encrypt(block);
            f(3);
            self.keyround(5).encrypt(block);
            f(4);
            self.keyround(6).encrypt(block);
            f(5);
            self.keyround(7).encrypt(block);
            f(6);
            self.keyround(8).encrypt(block);
            f(7);
            self.keyround(9).encrypt(block);
            f(8);
            self.keyround(10).encrypt_finish(block);
        }
    }

    impl<Blk, T> Decrypt for Key<T>
    where
        T: DecryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;
        const ROUNDS: usize = ROUNDS;

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Blk>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            f: F,
        ) {
            aes128::DecryptionKey::decrypt_interleaved(&self.0, block, f)
        }
    }

    pub trait DecryptionKey {
        type Block;
        type KeyRound: KeyRound<Block = Self::Block>;

        fn keyround(&self, index: usize) -> &Self::KeyRound;

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            // NOTE: instead of looping here, manually unroll the loop so the CPU has a large run of instructions
            //       without any branches.
            self.keyround(10).xor(block);
            self.keyround(9).decrypt(block);
            f(0);
            self.keyround(8).decrypt(block);
            f(1);
            self.keyround(7).decrypt(block);
            f(2);
            self.keyround(6).decrypt(block);
            f(3);
            self.keyround(5).decrypt(block);
            f(4);
            self.keyround(4).decrypt(block);
            f(5);
            self.keyround(3).decrypt(block);
            f(6);
            self.keyround(2).decrypt(block);
            f(7);
            self.keyround(1).decrypt(block);
            f(8);
            self.keyround(0).decrypt_finish(block);
        }
    }
}

pub mod aes256 {
    use super::*;

    pub const KEY_LEN: usize = 32;
    // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4548
    // mov	\$13,$bits			# 14 rounds for 256
    pub const ROUNDS: usize = 14;

    #[derive(Zeroize)]
    pub struct Key<T>(pub T);

    impl<Blk, T> Encrypt for Key<T>
    where
        T: EncryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;
        const ROUNDS: usize = ROUNDS;

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Blk>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            f: F,
        ) {
            aes256::EncryptionKey::encrypt_interleaved(&self.0, block, f)
        }
    }

    pub trait EncryptionKey {
        type Block;
        type KeyRound: KeyRound<Block = Self::Block>;

        fn keyround(&self, index: usize) -> &Self::KeyRound;

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            // NOTE: instead of looping here, manually unroll the loop so the CPU has a large run of instructions
            //       without any branches.
            self.keyround(0).xor(block);
            self.keyround(1).encrypt(block);
            f(0);
            self.keyround(2).encrypt(block);
            f(1);
            self.keyround(3).encrypt(block);
            f(2);
            self.keyround(4).encrypt(block);
            f(3);
            self.keyround(5).encrypt(block);
            f(4);
            self.keyround(6).encrypt(block);
            f(5);
            self.keyround(7).encrypt(block);
            f(6);
            self.keyround(8).encrypt(block);
            f(7);
            self.keyround(9).encrypt(block);
            f(8);
            self.keyround(10).encrypt(block);
            f(9);
            self.keyround(11).encrypt(block);
            f(10);
            self.keyround(12).encrypt(block);
            f(11);
            self.keyround(13).encrypt(block);
            f(12);
            self.keyround(14).encrypt_finish(block);
        }
    }

    impl<Blk, T> Decrypt for Key<T>
    where
        T: DecryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;
        const ROUNDS: usize = ROUNDS;

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Blk>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            f: F,
        ) {
            aes256::DecryptionKey::decrypt_interleaved(&self.0, block, f)
        }
    }

    pub trait DecryptionKey {
        type Block;
        type KeyRound: KeyRound<Block = Self::Block>;

        fn keyround(&self, index: usize) -> &Self::KeyRound;

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            // NOTE: instead of looping here, manually unroll the loop so the CPU has a large run of instructions
            //       without any branches.
            self.keyround(14).xor(block);
            self.keyround(13).decrypt(block);
            f(0);
            self.keyround(12).decrypt(block);
            f(1);
            self.keyround(11).decrypt(block);
            f(2);
            self.keyround(10).decrypt(block);
            f(3);
            self.keyround(9).decrypt(block);
            f(4);
            self.keyround(8).decrypt(block);
            f(5);
            self.keyround(7).decrypt(block);
            f(6);
            self.keyround(6).decrypt(block);
            f(7);
            self.keyround(5).decrypt(block);
            f(8);
            self.keyround(4).decrypt(block);
            f(9);
            self.keyround(3).decrypt(block);
            f(10);
            self.keyround(2).decrypt(block);
            f(11);
            self.keyround(1).decrypt(block);
            f(12);
            self.keyround(0).decrypt_finish(block);
        }
    }
}

#[cfg(test)]
mod tests;
