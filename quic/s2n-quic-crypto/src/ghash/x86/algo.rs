// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    arch::*,
    block::{Block, Zeroed},
};

// https://github.com/awslabs/aws-lc/blob/5833176448d48aff0c2dc4c1ab745649c769a7a6/crypto/cipher_extra/asm/aes128gcmsiv-x86_64.pl#L58
// poly:
// .quad 0x1, 0xc200000000000000
const POLYNOMIAL: __m128i = unsafe { core::mem::transmute([0x1u64, 0xc200000000000000]) };

// From https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/asm/ghash-x86_64.pl#L717
#[inline(always)]
// This implementation is written to closely follow the original code
#[allow(unknown_lints, clippy::needless_late_init)]
pub unsafe fn init(mut h: __m128i) -> __m128i {
    let mut t1;
    let t2;
    let mut t3;

    // # <<1 twist
    // vpshufd		\$0b11111111,$Hkey,$T2	# broadcast uppermost dword
    t2 = _mm_shuffle_epi32(h, 0b11111111);
    // vpsrlq		\$63,$Hkey,$T1
    t1 = _mm_srli_epi64(h, 63);
    // vpsllq		\$1,$Hkey,$Hkey
    h = _mm_slli_epi64(h, 1);
    // vpxor		$T3,$T3,$T3		#
    t3 = __m128i::zeroed();
    // vpcmpgtd	$T2,$T3,$T3		# broadcast carry bit
    t3 = _mm_cmpgt_epi32(t3, t2);
    // vpslldq		\$8,$T1,$T1
    t1 = _mm_slli_si128(t1, 8);
    // vpor		$T1,$Hkey,$Hkey		# H<<=1
    h = _mm_or_si128(h, t1);

    // # magic reduction
    // vpand		.L0x1c2_polynomial(%rip),$T3,$T3
    t3 = _mm_and_si128(t3, POLYNOMIAL);
    // vpxor		$T3,$Hkey,$Hkey		# if(carry) H^=0x1c2_polynomial
    h = h.xor(t3);

    h
}

// From https://github.com/awslabs/aws-lc/blob/5833176448d48aff0c2dc4c1ab745649c769a7a6/crypto/cipher_extra/asm/aes128gcmsiv-x86_64.pl#L93
#[inline(always)]
// This implementation is written to closely follow the original code
#[allow(unknown_lints, clippy::needless_late_init)]
pub unsafe fn gfmul(a: __m128i, b: __m128i) -> __m128i {
    // #########################
    // # a = T
    let t = a;
    // # b = TMP0 - remains unchanged
    let tmp0 = b;
    // # res = T
    // # uses also TMP1,TMP2,TMP3,TMP4
    // # __m128i GFMUL(__m128i A, __m128i B);

    // my $T = "%xmm0";
    // my $TMP0 = "%xmm1";
    // my $TMP1 = "%xmm2";
    let mut tmp1;
    // my $TMP2 = "%xmm3";
    let mut tmp2;
    // my $TMP3 = "%xmm4";
    let mut tmp3;
    // my $TMP4 = "%xmm5";
    let mut tmp4;

    // vpclmulqdq  \$0x00, $TMP0, $T, $TMP1
    tmp1 = _mm_clmulepi64_si128(t, tmp0, 0x00);
    // vpclmulqdq  \$0x11, $TMP0, $T, $TMP4
    tmp4 = _mm_clmulepi64_si128(t, tmp0, 0x11);
    // vpclmulqdq  \$0x10, $TMP0, $T, $TMP2
    tmp2 = _mm_clmulepi64_si128(t, tmp0, 0x10);
    // vpclmulqdq  \$0x01, $TMP0, $T, $TMP3
    tmp3 = _mm_clmulepi64_si128(t, tmp0, 0x01);
    // vpxor       $TMP3, $TMP2, $TMP2
    tmp2 = tmp2.xor(tmp3);
    // vpslldq     \$8, $TMP2, $TMP3
    tmp3 = _mm_slli_si128(tmp2, 8);
    // vpsrldq     \$8, $TMP2, $TMP2
    tmp2 = _mm_srli_si128(tmp2, 8);
    // vpxor       $TMP3, $TMP1, $TMP1
    tmp1 = tmp1.xor(tmp3);
    // vpxor       $TMP2, $TMP4, $TMP4
    tmp4 = tmp4.xor(tmp2);

    reduce(tmp1, tmp4)
}

/// Reduction phase of gfmul
// From https://github.com/awslabs/aws-lc/blob/5833176448d48aff0c2dc4c1ab745649c769a7a6/crypto/cipher_extra/asm/aes128gcmsiv-x86_64.pl#L93
#[inline(always)]
// This implementation is written to closely follow the original code
#[allow(unknown_lints, clippy::needless_late_init)]
pub unsafe fn reduce(mut tmp1: __m128i, tmp4: __m128i) -> __m128i {
    // my $T = "%xmm0";
    let t;
    // my $TMP0 = "%xmm1";
    // my $TMP1 = "%xmm2";
    // my $TMP2 = "%xmm3";
    let mut tmp2;
    // my $TMP3 = "%xmm4";
    let mut tmp3;
    // my $TMP4 = "%xmm5";

    // vpclmulqdq  \$0x10, poly(%rip), $TMP1, $TMP2
    tmp2 = _mm_clmulepi64_si128(tmp1, POLYNOMIAL, 0x10);
    // vpshufd     \$78, $TMP1, $TMP3
    tmp3 = _mm_shuffle_epi32(tmp1, 78);
    // vpxor       $TMP3, $TMP2, $TMP1
    tmp1 = tmp2.xor(tmp3);
    // vpclmulqdq  \$0x10, poly(%rip), $TMP1, $TMP2
    tmp2 = _mm_clmulepi64_si128(tmp1, POLYNOMIAL, 0x10);
    // vpshufd     \$78, $TMP1, $TMP3
    tmp3 = _mm_shuffle_epi32(tmp1, 78);
    // vpxor       $TMP3, $TMP2, $TMP1
    tmp1 = tmp2.xor(tmp3);
    // vpxor       $TMP4, $TMP1, $T
    t = tmp1.xor(tmp4);
    // ret
    t
}
