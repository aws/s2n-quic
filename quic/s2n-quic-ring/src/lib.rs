#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

mod ciphersuite;
#[macro_use]
mod negotiated;

pub use ring::{
    aead::{Algorithm, MAX_TAG_LEN},
    hkdf::Prk,
};
use s2n_quic_core::crypto::CryptoSuite;

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

impl CryptoSuite for RingCryptoSuite {
    type HandshakeCrypto = handshake::RingHandshakeCrypto;
    type InitialCrypto = initial::RingInitialCrypto;
    type OneRTTCrypto = one_rtt::RingOneRTTCrypto;
    type ZeroRTTCrypto = zero_rtt::RingZeroRTTCrypto;
    type RetryCrypto = retry::RingRetryCrypto;
}
