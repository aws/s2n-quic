// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

mod ciphersuite;
#[macro_use]
mod negotiated;
#[macro_use]
mod header_key;

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
pub struct RingCryptoSuite;

impl s2n_quic_core::crypto::CryptoSuite for RingCryptoSuite {
    type HandshakeKey = handshake::RingHandshakeKey;
    type HandshakeHeaderKey = handshake::RingHandshakeHeaderKey;
    type InitialKey = initial::RingInitialKey;
    type InitialHeaderKey = initial::RingInitialHeaderKey;
    type OneRttKey = one_rtt::RingOneRttKey;
    type OneRttHeaderKey = one_rtt::RingOneRttHeaderKey;
    type ZeroRttKey = zero_rtt::RingZeroRttKey;
    type ZeroRttHeaderKey = zero_rtt::RingZeroRttHeaderKey;
    type RetryKey = retry::RingRetryKey;
}
