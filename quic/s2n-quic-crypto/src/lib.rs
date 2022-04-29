// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

#[macro_use]
mod negotiated;
#[macro_use]
mod header_key;

mod aead;
mod aes;
mod aesgcm;
mod arch;
mod block;
mod cipher_suite;
mod ctr;
mod ghash;
mod iv;

#[doc(hidden)]
pub use ring::{
    self,
    aead::{Algorithm, MAX_TAG_LEN},
    hkdf::Prk,
};

#[derive(Clone)]
pub struct SecretPair {
    pub server: Prk,
    pub client: Prk,
}

pub mod handshake;
pub mod initial;
pub mod one_rtt;
pub mod retry;
pub mod zero_rtt;

#[derive(Clone, Copy, Debug, Default)]
pub struct Suite;

impl s2n_quic_core::crypto::CryptoSuite for Suite {
    type HandshakeKey = handshake::HandshakeKey;
    type HandshakeHeaderKey = handshake::HandshakeHeaderKey;
    type InitialKey = initial::InitialKey;
    type InitialHeaderKey = initial::InitialHeaderKey;
    type OneRttKey = one_rtt::OneRttKey;
    type OneRttHeaderKey = one_rtt::OneRttHeaderKey;
    type ZeroRttKey = zero_rtt::ZeroRttKey;
    type ZeroRttHeaderKey = zero_rtt::ZeroRttHeaderKey;
    type RetryKey = retry::RetryKey;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing;
