// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aes,
    arch::*,
    block::{BatchMut, Block, Zeroed},
};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub struct Key<const ROUNDS: usize> {
    pub encrypt: EncryptionKey<ROUNDS>,
    pub decrypt: DecryptionKey<ROUNDS>,
}

impl<const N: usize> super::aes128::EncryptionKey for Key<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        self.encrypt.keyround(index)
    }
}

impl<const N: usize> super::aes128::DecryptionKey for Key<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        self.decrypt.keyround(index)
    }
}

impl<const N: usize> super::aes256::EncryptionKey for Key<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        self.encrypt.keyround(index)
    }
}

impl<const N: usize> super::aes256::DecryptionKey for Key<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        self.decrypt.keyround(index)
    }
}

pub struct EncryptionKey<const ROUNDS: usize>([KeyRound; ROUNDS]);

impl<const N: usize> super::aes128::EncryptionKey for EncryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        unsafe {
            unsafe_assert!(index < N);
            self.0.get_unchecked(index)
        }
    }
}

impl<const N: usize> super::aes256::EncryptionKey for EncryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        unsafe {
            unsafe_assert!(index < N);
            self.0.get_unchecked(index)
        }
    }
}

pub struct DecryptionKey<const ROUNDS: usize>([KeyRound; ROUNDS]);

impl<const N: usize> super::aes128::DecryptionKey for DecryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        if index == 0 {
            return &self.0[10];
        }
        unsafe {
            unsafe_assert!(index >= (N - 1));
            let index = index - (N - 1);
            self.0.get_unchecked(index)
        }
    }
}

impl<const N: usize> super::aes256::DecryptionKey for DecryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        if index == 0 {
            return &self.0[14];
        }
        unsafe {
            unsafe_assert!(index >= (N - 1));
            let index = index - (N - 1);
            self.0.get_unchecked(index)
        }
    }
}

pub mod aes128 {
    use super::*;
    use crate::aes::aes128::KEY_LEN;

    const ROUNDS: usize = aes::aes128::ROUNDS + 1;

    pub type Key = super::Key<ROUNDS>;
    pub type EncryptionKey = super::EncryptionKey<ROUNDS>;

    impl Key {
        #[inline(always)]
        pub fn new(key: [u8; KEY_LEN]) -> Self {
            let mut enc = [KeyRound(__m128i::zeroed()); ROUNDS];
            let mut dec = [KeyRound(__m128i::zeroed()); ROUNDS];

            unsafe {
                debug_assert!(Avx2::is_supported());

                // _mm_aeskeygenassist_si128 requires the second argument to be a constant
                // so we need to use a macro rather than a function
                macro_rules! keyround {
                    ($round:expr, $imm8:expr) => {{
                        let key = enc[$round].0;
                        let gen = _mm_aeskeygenassist_si128(key, $imm8);
                        let gen = _mm_shuffle_epi32(gen, 255);
                        let key = key.xor(_mm_slli_si128(key, 4));
                        let key = key.xor(_mm_slli_si128(key, 8));
                        let key = key.xor(gen);
                        enc[$round + 1] = KeyRound(key);
                    }};
                }

                // initialize the keys with the provided key
                enc[0] = KeyRound(__m128i::from_array(key));

                // aes128 has 10 keyrounds
                keyround!(0, 0x01);
                keyround!(1, 0x02);
                keyround!(2, 0x04);
                keyround!(3, 0x08);
                keyround!(4, 0x10);
                keyround!(5, 0x20);
                keyround!(6, 0x40);
                keyround!(7, 0x80);
                keyround!(8, 0x1b);
                keyround!(9, 0x36);

                // initialize the decryption half
                dec[0] = enc[10];
                dec[1] = enc[9].inv_mix_columns();
                dec[2] = enc[8].inv_mix_columns();
                dec[3] = enc[7].inv_mix_columns();
                dec[4] = enc[6].inv_mix_columns();
                dec[5] = enc[5].inv_mix_columns();
                dec[6] = enc[4].inv_mix_columns();
                dec[7] = enc[3].inv_mix_columns();
                dec[8] = enc[2].inv_mix_columns();
                dec[9] = enc[1].inv_mix_columns();
                dec[10] = enc[0];
            }

            Self {
                encrypt: EncryptionKey(enc),
                decrypt: DecryptionKey(dec),
            }
        }
    }
}

pub mod aes256 {
    use super::*;
    use crate::aes::aes256::KEY_LEN;

    const ROUNDS: usize = aes::aes256::ROUNDS + 1;

    pub type Key = super::Key<ROUNDS>;
    pub type EncryptionKey = super::EncryptionKey<ROUNDS>;

    impl Key {
        #[inline(always)]
        pub fn new(key: [u8; KEY_LEN]) -> Self {
            let mut enc = [KeyRound(__m128i::zeroed()); ROUNDS];
            let mut dec = [KeyRound(__m128i::zeroed()); ROUNDS];

            unsafe {
                debug_assert!(Avx2::is_supported());

                // _mm_aeskeygenassist_si128 requires the second argument to be a constant
                // so we need to use a macro rather than a function
                macro_rules! keyround {
                    ($idx:expr, $imm8:expr) => {
                        let mut temp1 = enc[$idx - 2].0;
                        let mut temp2;
                        let mut temp3 = enc[$idx - 1].0;
                        let mut temp4;

                        temp2 = _mm_aeskeygenassist_si128(temp3, $imm8);
                        temp2 = _mm_shuffle_epi32(temp2, 0xff);
                        temp4 = _mm_slli_si128(temp1, 0x4);
                        temp1 = temp1.xor(temp4);
                        temp4 = _mm_slli_si128(temp4, 0x4);
                        temp1 = temp1.xor(temp4);
                        temp4 = _mm_slli_si128(temp4, 0x4);
                        temp1 = temp1.xor(temp4);
                        temp1 = temp1.xor(temp2);

                        enc[$idx] = KeyRound(temp1);

                        temp4 = _mm_aeskeygenassist_si128(temp1, 0x00);
                        temp2 = _mm_shuffle_epi32(temp4, 0xaa);
                        temp4 = _mm_slli_si128(temp3, 0x4);
                        temp3 = temp3.xor(temp4);
                        temp4 = _mm_slli_si128(temp4, 0x4);
                        temp3 = temp3.xor(temp4);
                        temp4 = _mm_slli_si128(temp4, 0x4);
                        temp3 = temp3.xor(temp4);
                        temp3 = temp3.xor(temp2);

                        enc[$idx + 1] = KeyRound(temp3);
                    };
                }

                let key = key.as_ptr() as *const __m128i;
                enc[0] = KeyRound(_mm_loadu_si128(key));
                enc[1] = KeyRound(_mm_loadu_si128(key.offset(1)));

                keyround!(2, 0x01);
                keyround!(4, 0x02);
                keyround!(6, 0x04);
                keyround!(8, 0x08);
                keyround!(10, 0x10);
                keyround!(12, 0x20);

                // final round
                {
                    let pos = 14;

                    let mut temp1 = enc[pos - 2].0;
                    let mut temp2;
                    let temp3 = enc[pos - 1].0;
                    let mut temp4;

                    temp2 = _mm_aeskeygenassist_si128(temp3, 0x40);
                    temp2 = _mm_shuffle_epi32(temp2, 0xff);
                    temp4 = _mm_slli_si128(temp1, 0x4);
                    temp1 = temp1.xor(temp4);
                    temp4 = _mm_slli_si128(temp4, 0x4);
                    temp1 = temp1.xor(temp4);
                    temp4 = _mm_slli_si128(temp4, 0x4);
                    temp1 = temp1.xor(temp4);
                    temp1 = temp1.xor(temp2);

                    enc[pos] = KeyRound(temp1);
                }

                // initialize the decryption half
                dec[0] = enc[14];
                dec[1] = enc[13].inv_mix_columns();
                dec[2] = enc[12].inv_mix_columns();
                dec[3] = enc[11].inv_mix_columns();
                dec[4] = enc[10].inv_mix_columns();
                dec[5] = enc[9].inv_mix_columns();
                dec[6] = enc[8].inv_mix_columns();
                dec[7] = enc[7].inv_mix_columns();
                dec[8] = enc[6].inv_mix_columns();
                dec[9] = enc[5].inv_mix_columns();
                dec[10] = enc[4].inv_mix_columns();
                dec[11] = enc[3].inv_mix_columns();
                dec[12] = enc[2].inv_mix_columns();
                dec[13] = enc[1].inv_mix_columns();
                dec[14] = enc[0];
            }

            Self {
                encrypt: EncryptionKey(enc),
                decrypt: DecryptionKey(dec),
            }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct KeyRound(__m128i);

impl KeyRound {
    fn inv_mix_columns(self) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            Self(_mm_aesimc_si128(self.0))
        }
    }
}

impl super::KeyRound for KeyRound {
    type Block = __m128i;

    #[inline(always)]
    fn xor<B: BatchMut<Block = __m128i>>(&self, block: &mut B) {
        block.update(|_idx, b| *b = b.xor(self.0));
    }

    #[inline(always)]
    fn encrypt<B: BatchMut<Block = __m128i>>(&self, block: &mut B) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            block.update(|_idx, b| *b = _mm_aesenc_si128(*b, self.0));
        }
    }

    #[inline(always)]
    fn encrypt_finish<B: BatchMut<Block = __m128i>>(&self, block: &mut B) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            block.update(|_idx, b| *b = _mm_aesenclast_si128(*b, self.0));
        }
    }

    #[inline(always)]
    fn decrypt<B: BatchMut<Block = __m128i>>(&self, block: &mut B) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            block.update(|_idx, b| *b = _mm_aesdec_si128(*b, self.0));
        }
    }

    #[inline(always)]
    fn decrypt_finish<B: BatchMut<Block = __m128i>>(&self, block: &mut B) {
        unsafe {
            debug_assert!(Avx2::is_supported());
            block.update(|_idx, b| *b = _mm_aesdeclast_si128(*b, self.0));
        }
    }
}
