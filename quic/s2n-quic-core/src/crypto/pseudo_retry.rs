use hex_literal::hex;

//= https://tools.ietf.org/id/draft-ietf-quic-tls-31.txt#5.8
//# The Retry Integrity Tag is a 128-bit field that is computed as the
//# output of AEAD_AES_128_GCM ([AEAD]) used with the following inputs:
//#
//# *  The secret key, K, is 128 bits equal to
//#    0xccce187ed09a09d05728155a6cb96be1.
//#
pub const SECRET_KEY: [u8; 16] = hex!("ccce187ed09a09d05728155a6cb96be1");

//= https://tools.ietf.org/id/draft-ietf-quic-tls-31.txt#5.8
//#   *  The nonce, N, is 96 bits equal to 0xe54930f97f2136f0530a8c1c.

pub const NONCE: [u8; 12] = hex!("e54930f97f2136f0530a8c1c");

/// Packets implementing this trait can calculate retry integrity tags.
pub trait RetryCrypto {
    fn calculate_tag() -> [u8; 16];
    fn is_valid() -> bool;
}
