use crate::crypto::CryptoError;
use hex_literal::hex;

pub const INTEGRITY_TAG_LEN: usize = 16;
pub type IntegrityTag = [u8; INTEGRITY_TAG_LEN];

pub trait RetryCrypto {
    fn generate_tag(payload: &[u8]) -> IntegrityTag;
    fn validate(payload: &[u8], tag: IntegrityTag) -> Result<(), CryptoError>;
}

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

pub mod example {
    use super::*;

    pub const PACKET_LEN: usize = 36;

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A.4
    //# This shows a Retry packet that might be sent in response to the
    //# Initial packet in Appendix A.2.  The integrity check includes the
    //# client-chosen connection ID value of 0x8394c8f03e515708, but that
    //# value is not included in the final Retry packet:
    //#
    //# ffff0000200008f067a5502a4262b574 6f6b656e59756519dd6cc85bd90e33a9
    //# 34d2ff85
    pub const PACKET: [u8; PACKET_LEN] = hex!(
        "
    ff ff000020 00 08 f067a5502a4262b5 746f6b656e59756519dd6cc85bd90e33a9
    34d2ff85
    "
    );

    pub const PSEUDO_PACKET: [u8; 29] =
        hex!("088394c8f03e515708 ffff000020 00 08f067a5502a4262b5 746f6b656e");

    pub const EXPECTED_TAG: [u8; 16] = hex!("59756519dd6cc85bd90e33a934d2ff85");

    // The server sends an empty destination connection ID back to the client
    pub const DCID: [u8; 0] = hex!("");

    // This is the destination connection generated locally in the server
    // The Retry Packet should have this as the source connection ID
    pub const SCID: [u8; 8] = hex!("f067a5502a4262b5");

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#A
    //# These packets use an 8-byte client-chosen Destination Connection ID
    //# of 0x8394c8f03e515708.

    pub const ODCID: [u8; 8] = hex!("8394c8f03e515708");

    pub const VERSION: u32 = 0xff00_0020;

    pub const TOKEN: [u8; 5] = hex!("746f6b656e");

    pub const TOKEN_LEN: usize = 5;
}
