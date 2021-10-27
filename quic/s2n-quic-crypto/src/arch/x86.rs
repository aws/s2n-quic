// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::arch::Arch;
use lazy_static::lazy_static;

lazy_static! {
    static ref IS_SUPPORTED: bool = is_x86_feature_detected!("aes")
        && is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("pclmulqdq");
}

pub struct Avx2;

impl Arch for Avx2 {
    #[inline(always)]
    fn is_supported() -> bool {
        *IS_SUPPORTED
    }

    #[target_feature(enable = "aes,avx2,pclmulqdq")]
    #[inline]
    unsafe fn call<F: FnOnce() -> R, R>(f: F) -> R {
        debug_assert!(Self::is_supported());
        f()
    }
}
