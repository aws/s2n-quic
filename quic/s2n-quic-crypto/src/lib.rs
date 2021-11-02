// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// Since this crate doesn't actually export anything at the moment, don't emit warnings for dead code
#![cfg_attr(not(test), allow(dead_code))]

/// Asserts that a boolean expression is true at runtime, only if debug_assertions are enabled.
///
/// Otherwise, the compiler is told to assume that the expression is always true and can perform
/// additional optimizations.
macro_rules! unsafe_assert {
    ($cond:expr) => {
        unsafe_assert!($cond, "assumption failed: {}", stringify!($cond));
    };
    ($cond:expr $(, $fmtarg:expr)* $(,)?) => {
        let v = $cond;

        debug_assert!(v $(, $fmtarg)*);
        if cfg!(not(debug_assertions)) && !v {
            core::hint::unreachable_unchecked();
        }
    };
}

mod aes;
mod aesgcm;
mod arch;
mod block;
mod ctr;
mod ghash;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
