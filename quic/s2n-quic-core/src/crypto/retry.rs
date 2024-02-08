// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::packet_protection;
use hex_literal::hex;

pub const INTEGRITY_TAG_LEN: usize = 16;
pub type IntegrityTag = [u8; INTEGRITY_TAG_LEN];

pub trait RetryKey {
    fn generate_tag(payload: &[u8]) -> IntegrityTag;
    fn validate(payload: &[u8], tag: IntegrityTag) -> Result<(), packet_protection::Error>;
}

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.8
//# The Retry Integrity Tag is a 128-bit field that is computed as the
//# output of AEAD_AES_128_GCM [AEAD] used with the following inputs:
//#
//# *  The secret key, K, is 128 bits equal to
//#    0xbe0c690b9f66575a1d766b54e368c84e.
//#
pub const SECRET_KEY_BYTES: [u8; 16] = hex!("be0c690b9f66575a1d766b54e368c84e");

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.8
//#   *  The nonce, N, is 96 bits equal to 0x461599d35d632bf2239825bb.

pub const NONCE_BYTES: [u8; 12] = hex!("461599d35d632bf2239825bb");

pub mod example {
    use super::*;

    pub const INVALID_PACKET_NO_TOKEN_LEN: usize = 31;
    pub const INVALID_PACKET_NO_TOKEN: [u8; INVALID_PACKET_NO_TOKEN_LEN] = hex!(
        "
        ff 00000001 00 08 f067a5502a4262b5 59756519dd6cc85bd90e33a9
        34d2ff85
        "
    );
    pub const PACKET_LEN: usize = 36;

    //= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.4
    //# This shows a Retry packet that might be sent in response to the
    //# Initial packet in Appendix A.2.  The integrity check includes the
    //# client-chosen connection ID value of 0x8394c8f03e515708, but that
    //# value is not included in the final Retry packet:
    //#
    //# ff000000010008f067a5502a4262b574 6f6b656e04a265ba2eff4d829058fb3f
    //# 0f2496ba
    pub const PACKET: [u8; PACKET_LEN] = hex!(
        "
        ff000000010008f067a5502a4262b574 6f6b656e04a265ba2eff4d829058fb3f
        0f2496ba
        "
    );

    pub const PSEUDO_PACKET: [u8; 29] =
        hex!("088394c8f03e515708 ff00000001 00 08f067a5502a4262b5 746f6b656e");

    pub const EXPECTED_TAG: [u8; 16] = hex!("04a265ba2eff4d829058fb3f0f2496ba");

    // The server sends an empty destination connection ID back to the client
    pub const DCID: [u8; 0] = hex!("");

    // This is the destination connection generated locally in the server
    // The Retry Packet should have this as the source connection ID
    pub const SCID: [u8; 8] = hex!("f067a5502a4262b5");

    //= https://www.rfc-editor.org/rfc/rfc9001#appendix-A
    //# These packets use an 8-byte client-chosen Destination Connection ID
    //# of 0x8394c8f03e515708.

    pub const ODCID: [u8; 8] = hex!("8394c8f03e515708");

    pub const VERSION: u32 = 0x1;

    pub const TOKEN: [u8; 5] = hex!("746f6b656e");

    pub const TOKEN_LEN: usize = 5;
}
