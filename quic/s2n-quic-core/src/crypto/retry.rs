use crate::crypto::{CryptoError, HeaderCrypto, Key};
use hex_literal::hex;

pub type IntegrityTag = [u8; 16];

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.8
//# The Retry Integrity Tag is a 128-bit field that is computed as the
//# output of AEAD_AES_128_GCM ([AEAD]) used with the following inputs:
//#
//# *  The secret key, K, is 128 bits equal to
//#    0xccce187ed09a09d05728155a6cb96be1.
//#
pub const SECRET_KEY_BYTES: [u8; 16] = hex!("ccce187ed09a09d05728155a6cb96be1");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.8
//#   *  The nonce, N, is 96 bits equal to 0xe54930f97f2136f0530a8c1c.

pub const NONCE_BYTES: [u8; 12] = hex!("e54930f97f2136f0530a8c1c");

/// Types for which are able to perform cryptography.
///
/// This marker trait ensures only 1-RTT-level keys
/// are used with Short packets. Any key misuses are
/// caught by the type system.
pub trait RetryCrypto: Key + HeaderCrypto {
    fn generate_tag(payload: &[u8]) -> IntegrityTag;
    fn validate(payload: &[u8], tag: IntegrityTag) -> Result<(), CryptoError>;
}
