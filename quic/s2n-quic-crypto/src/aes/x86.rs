// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aes,
    arch::*,
    block::{BatchMut, Block, Zeroed},
};
use s2n_quic_core::assume;
use zeroize::Zeroize;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[derive(Zeroize)]
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

#[derive(Zeroize)]
pub struct EncryptionKey<const ROUNDS: usize>([KeyRound; ROUNDS]);

impl<const N: usize> super::aes128::EncryptionKey for EncryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        unsafe {
            assume!(index < N);
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
            assume!(index < N);
            self.0.get_unchecked(index)
        }
    }
}

#[derive(Zeroize)]
pub struct DecryptionKey<const ROUNDS: usize>([KeyRound; ROUNDS]);

impl<const N: usize> super::aes128::DecryptionKey for DecryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        unsafe {
            assume!(index < N);
            self.0.get_unchecked(index)
        }
    }
}

impl<const N: usize> super::aes256::DecryptionKey for DecryptionKey<N> {
    type Block = __m128i;
    type KeyRound = KeyRound;

    #[inline(always)]
    fn keyround(&self, index: usize) -> &Self::KeyRound {
        unsafe {
            assume!(index < N);
            self.0.get_unchecked(index)
        }
    }
}

/// Calls the keygen assist intrinsic with the same argument order as the assembly
///
/// By having it in the same order, it should make comparing the code to other implementations
/// a bit more straightforward.
macro_rules! aeskeygenassist {
    ($imm8:expr, $src:expr, $dest:ident) => {
        $dest = _mm_aeskeygenassist_si128($src, $imm8);
    };
}

macro_rules! key_shuffle {
    ($dest:ident) => {{
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4703
        // shufps	\$0b00010000,%xmm0,%xmm4
        // xorps	%xmm4,%xmm0
        // shufps	\$0b10001100,%xmm0,%xmm4
        // xorps	%xmm4,%xmm0

        let mut xmm4 = _mm_slli_si128($dest, 4);
        $dest = xmm4.xor($dest);
        xmm4 = _mm_slli_si128($dest, 8);
        $dest = xmm4.xor($dest);
    }}
}

pub mod aes128 {
    use super::*;
    use crate::aes::aes128::KEY_LEN;

    const ROUNDS: usize = aes::aes128::ROUNDS + 1;

    pub type Key = super::Key<ROUNDS>;
    pub type EncryptionKey = super::EncryptionKey<ROUNDS>;

    impl Key {
        #[inline(always)]
        // This implementation is written to closely follow the original code
        #[allow(unknown_lints, clippy::needless_late_init)]
        pub fn new(key: [u8; KEY_LEN]) -> Self {
            let mut enc = [KeyRound(__m128i::zeroed()); ROUNDS];
            let mut dec = [KeyRound(__m128i::zeroed()); ROUNDS];

            unsafe {
                debug_assert!(Avx2::is_supported());

                let mut xmm0;
                let mut xmm1;

                // _mm_aeskeygenassist_si128 requires the second argument to be a constant
                // so we need to use a macro rather than a function
                macro_rules! key_expansion_128 {
                    ($idx:expr) => {{
                        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4661-L4666
                        key_shuffle!(xmm0);
                        // shufps	\$0b11111111,%xmm1,%xmm1	# critical path
                        xmm1 = _mm_shuffle_epi32(xmm1, 0b11111111);
                        // xorps	%xmm1,%xmm0
                        xmm0 = xmm1.xor(xmm0);

                        enc[$idx] = KeyRound(xmm0);
                    }};
                }

                // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4382
                // $movkey	%xmm0,($key)			# round 0
                xmm0 = __m128i::from_array(key);
                enc[0] = KeyRound(xmm0);
                // aeskeygenassist	\$0x1,%xmm0,%xmm1	# round 1
                aeskeygenassist!(0x1, xmm0, xmm1);
                // call		.Lkey_expansion_128_cold
                key_expansion_128!(1);
                // aeskeygenassist	\$0x2,%xmm0,%xmm1	# round 2
                aeskeygenassist!(0x2, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(2);
                // aeskeygenassist	\$0x4,%xmm0,%xmm1	# round 3
                aeskeygenassist!(0x4, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(3);
                // aeskeygenassist	\$0x8,%xmm0,%xmm1	# round 4
                aeskeygenassist!(0x8, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(4);
                // aeskeygenassist	\$0x10,%xmm0,%xmm1	# round 5
                aeskeygenassist!(0x10, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(5);
                // aeskeygenassist	\$0x20,%xmm0,%xmm1	# round 6
                aeskeygenassist!(0x20, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(6);
                // aeskeygenassist	\$0x40,%xmm0,%xmm1	# round 7
                aeskeygenassist!(0x40, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(7);
                // aeskeygenassist	\$0x80,%xmm0,%xmm1	# round 8
                aeskeygenassist!(0x80, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(8);
                // aeskeygenassist	\$0x1b,%xmm0,%xmm1	# round 9
                aeskeygenassist!(0x1b, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(9);
                // aeskeygenassist	\$0x36,%xmm0,%xmm1	# round 10
                aeskeygenassist!(0x36, xmm0, xmm1);
                // call		.Lkey_expansion_128
                key_expansion_128!(10);

                // initialize the decryption half
                dec[10] = enc[10];
                dec[9] = enc[9].inv_mix_columns();
                dec[8] = enc[8].inv_mix_columns();
                dec[7] = enc[7].inv_mix_columns();
                dec[6] = enc[6].inv_mix_columns();
                dec[5] = enc[5].inv_mix_columns();
                dec[4] = enc[4].inv_mix_columns();
                dec[3] = enc[3].inv_mix_columns();
                dec[2] = enc[2].inv_mix_columns();
                dec[1] = enc[1].inv_mix_columns();
                dec[0] = enc[0];
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
        // This implementation is written to closely follow the original code
        #[allow(unknown_lints, clippy::needless_late_init)]
        pub fn new(key: [u8; KEY_LEN]) -> Self {
            let mut enc = [KeyRound(__m128i::zeroed()); ROUNDS];
            let mut dec = [KeyRound(__m128i::zeroed()); ROUNDS];

            unsafe {
                debug_assert!(Avx2::is_supported());

                // To make it easier to compare the libcrypto version, name the temporary variables
                // the same as the registers.
                let mut xmm0;
                let mut xmm1;
                let mut xmm2;

                macro_rules! key_expansion_256a {
                    ($idx:expr) => {{
                        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4703
                        key_shuffle!(xmm0);
                        // shufps	\$0b11111111,%xmm1,%xmm1	# critical path
                        xmm1 = _mm_shuffle_epi32(xmm1, 0b11111111);
                        // xorps	%xmm1,%xmm0
                        xmm0 = xmm0.xor(xmm1);

                        enc[$idx] = KeyRound(xmm0);
                    }};
                }

                macro_rules! key_expansion_256b {
                    ($idx:expr) => {{
                        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4713
                        key_shuffle!(xmm2);
                        // shufps	\$0b10101010,%xmm1,%xmm1	# critical path
                        xmm1 = _mm_shuffle_epi32(xmm1, 0b10101010);
                        // xorps	%xmm1,%xmm2
                        xmm2 = xmm2.xor(xmm1);

                        enc[$idx] = KeyRound(xmm2);
                    }};
                }

                let key = key.as_ptr() as *const __m128i;

                // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/asm/aesni-x86_64.pl#L4553
                // $movkey	%xmm0,($key)			# round 0
                xmm0 = _mm_loadu_si128(key);
                enc[0] = KeyRound(xmm0);
                // $movkey	%xmm2,16($key)			# round 1
                xmm2 = _mm_loadu_si128(key.offset(1));
                enc[1] = KeyRound(xmm2);

                // aeskeygenassist	\$0x1,%xmm2,%xmm1	# round 2
                aeskeygenassist!(0x1, xmm2, xmm1);
                // call		.Lkey_expansion_256a_cold
                key_expansion_256a!(2);
                // aeskeygenassist	\$0x1,%xmm0,%xmm1	# round 3
                aeskeygenassist!(0x1, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(3);
                // aeskeygenassist	\$0x2,%xmm2,%xmm1	# round 4
                aeskeygenassist!(0x2, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(4);
                // aeskeygenassist	\$0x2,%xmm0,%xmm1	# round 5
                aeskeygenassist!(0x2, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(5);
                // aeskeygenassist	\$0x4,%xmm2,%xmm1	# round 6
                aeskeygenassist!(0x4, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(6);
                // aeskeygenassist	\$0x4,%xmm0,%xmm1	# round 7
                aeskeygenassist!(0x4, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(7);
                // aeskeygenassist	\$0x8,%xmm2,%xmm1	# round 8
                aeskeygenassist!(0x8, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(8);
                // aeskeygenassist	\$0x8,%xmm0,%xmm1	# round 9
                aeskeygenassist!(0x8, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(9);
                // aeskeygenassist	\$0x10,%xmm2,%xmm1	# round 10
                aeskeygenassist!(0x10, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(10);
                // aeskeygenassist	\$0x10,%xmm0,%xmm1	# round 11
                aeskeygenassist!(0x10, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(11);
                // aeskeygenassist	\$0x20,%xmm2,%xmm1	# round 12
                aeskeygenassist!(0x20, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(12);
                // aeskeygenassist	\$0x20,%xmm0,%xmm1	# round 13
                aeskeygenassist!(0x20, xmm0, xmm1);
                // call		.Lkey_expansion_256b
                key_expansion_256b!(13);
                // aeskeygenassist	\$0x40,%xmm2,%xmm1	# round 14
                aeskeygenassist!(0x40, xmm2, xmm1);
                // call		.Lkey_expansion_256a
                key_expansion_256a!(14);

                // initialize the decryption half
                dec[14] = enc[14];
                dec[13] = enc[13].inv_mix_columns();
                dec[12] = enc[12].inv_mix_columns();
                dec[11] = enc[11].inv_mix_columns();
                dec[10] = enc[10].inv_mix_columns();
                dec[9] = enc[9].inv_mix_columns();
                dec[8] = enc[8].inv_mix_columns();
                dec[7] = enc[7].inv_mix_columns();
                dec[6] = enc[6].inv_mix_columns();
                dec[5] = enc[5].inv_mix_columns();
                dec[4] = enc[4].inv_mix_columns();
                dec[3] = enc[3].inv_mix_columns();
                dec[2] = enc[2].inv_mix_columns();
                dec[1] = enc[1].inv_mix_columns();
                dec[0] = enc[0];
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
    #[inline(always)]
    fn inv_mix_columns(self) -> Self {
        unsafe {
            debug_assert!(Avx2::is_supported());
            Self(_mm_aesimc_si128(self.0))
        }
    }
}

impl Default for KeyRound {
    #[inline(always)]
    fn default() -> Self {
        Self(__m128i::zeroed())
    }
}

impl zeroize::DefaultIsZeroes for KeyRound {}

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
