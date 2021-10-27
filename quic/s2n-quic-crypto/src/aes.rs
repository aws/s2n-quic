// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::block::BatchMut;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub trait Encrypt {
    type Block;
    const KEY_LEN: usize;

    fn encrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
    fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
        &self,
        block: &mut B,
        f: F,
    );
}

pub trait Decrypt {
    type Block;
    const KEY_LEN: usize;

    fn decrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B);
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

pub const BLOCK_LEN: usize = 16;

pub mod aes128 {
    use super::*;

    pub const KEY_LEN: usize = 16;
    pub const ROUNDS: usize = 10;

    pub struct Key<T>(pub T);

    impl<Blk, T> Encrypt for Key<T>
    where
        T: EncryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;

        #[inline(always)]
        fn encrypt<B: BatchMut<Block = Blk>>(&self, block: &mut B) {
            aes128::EncryptionKey::encrypt(&self.0, block)
        }

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
        fn encrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
            self.encrypt_interleaved(
                block,
                #[inline(always)]
                |_| {},
            );
        }

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
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

        #[inline(always)]
        fn decrypt<B: BatchMut<Block = Blk>>(&self, block: &mut B) {
            aes128::DecryptionKey::decrypt(&self.0, block)
        }

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
        fn decrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
            self.decrypt_interleaved(
                block,
                #[inline(always)]
                |_| {},
            );
        }

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            self.keyround(10).xor(block);
            self.keyround(11).decrypt(block);
            f(0);
            self.keyround(12).decrypt(block);
            f(1);
            self.keyround(13).decrypt(block);
            f(2);
            self.keyround(14).decrypt(block);
            f(3);
            self.keyround(15).decrypt(block);
            f(4);
            self.keyround(16).decrypt(block);
            f(5);
            self.keyround(17).decrypt(block);
            f(6);
            self.keyround(18).decrypt(block);
            f(7);
            self.keyround(19).decrypt(block);
            f(8);
            self.keyround(0).decrypt_finish(block);
        }
    }
}

pub mod aes256 {
    use super::*;

    pub const KEY_LEN: usize = 32;
    pub const ROUNDS: usize = 14;

    pub struct Key<T>(pub T);

    impl<Blk, T> Encrypt for Key<T>
    where
        T: EncryptionKey<Block = Blk>,
    {
        type Block = Blk;
        const KEY_LEN: usize = KEY_LEN;

        #[inline(always)]
        fn encrypt<B: BatchMut<Block = Blk>>(&self, block: &mut B) {
            aes256::EncryptionKey::encrypt(&self.0, block)
        }

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
        fn encrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
            self.encrypt_interleaved(
                block,
                #[inline(always)]
                |_| {},
            );
        }

        #[inline(always)]
        fn encrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
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

        #[inline(always)]
        fn decrypt<B: BatchMut<Block = Blk>>(&self, block: &mut B) {
            aes256::DecryptionKey::decrypt(&self.0, block)
        }

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
        fn decrypt<B: BatchMut<Block = Self::Block>>(&self, block: &mut B) {
            self.decrypt_interleaved(
                block,
                #[inline(always)]
                |_| {},
            );
        }

        #[inline(always)]
        fn decrypt_interleaved<B: BatchMut<Block = Self::Block>, F: FnMut(usize)>(
            &self,
            block: &mut B,
            mut f: F,
        ) {
            self.keyround(14).xor(block);
            self.keyround(15).decrypt(block);
            f(0);
            self.keyround(16).decrypt(block);
            f(1);
            self.keyround(17).decrypt(block);
            f(2);
            self.keyround(18).decrypt(block);
            f(3);
            self.keyround(19).decrypt(block);
            f(4);
            self.keyround(20).decrypt(block);
            f(5);
            self.keyround(21).decrypt(block);
            f(6);
            self.keyround(22).decrypt(block);
            f(7);
            self.keyround(23).decrypt(block);
            f(8);
            self.keyround(24).decrypt(block);
            f(9);
            self.keyround(25).decrypt(block);
            f(10);
            self.keyround(26).decrypt(block);
            f(11);
            self.keyround(27).decrypt(block);
            f(12);
            self.keyround(0).decrypt_finish(block);
        }
    }
}

#[cfg(test)]
mod tests;
