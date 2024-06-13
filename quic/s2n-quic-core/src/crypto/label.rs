// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use hex_literal::hex;

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# The labels generated during the execution of the HKDF-Expand-Label
//# function (that is, HkdfLabel.label) and part of the value given to
//# the HKDF-Expand function in order to produce its output are:
//#
//# client in:  00200f746c73313320636c69656e7420696e00

pub const CLIENT_IN: [u8; 19] = hex!("00200f746c73313320636c69656e7420696e00");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# server in:  00200f746c7331332073657276657220696e00

pub const SERVER_IN: [u8; 19] = hex!("00200f746c7331332073657276657220696e00");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# quic key:  00100e746c7331332071756963206b657900

pub const QUIC_KEY_16: [u8; 18] = hex!("00100e746c7331332071756963206b657900");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# quic iv:  000c0d746c733133207175696320697600

pub const QUIC_IV_12: [u8; 17] = hex!("000c0d746c733133207175696320697600");

//= https://www.rfc-editor.org/rfc/rfc9001#appendix-A.1
//# quic hp:  00100d746c733133207175696320687000

pub const QUIC_HP_16: [u8; 17] = hex!("00100d746c733133207175696320687000");

//= https://www.rfc-editor.org/rfc/rfc9001#section-6.1
//# Endpoints maintain separate read and write secrets for packet
//# protection.  An endpoint initiates a key update by updating its
//# packet protection write secret and using that to protect new packets.
//#
//# The endpoint creates a new write secret from the existing write
//# secret as performed in Section 7.2 of [TLS13].  This uses the KDF
//# function provided by TLS with a label of "quic ku".  The
//# corresponding key and IV are created from that secret as defined in
//# Section 5.1.  The header protection key is not updated.

pub const QUIC_KU_16: [u8; 17] = hex!("00100d746c7331332071756963206b7500");

// 32-byte labels

pub const QUIC_KEY_32: [u8; 18] = hex!("00200e746c7331332071756963206b657900");
pub const QUIC_HP_32: [u8; 17] = hex!("00200d746c733133207175696320687000");
pub const QUIC_KU_32: [u8; 17] = hex!("00200d746c7331332071756963206b7500");

// 48-byte labels
pub const QUIC_KU_48: [u8; 17] = hex!("00300d746c7331332071756963206b7500");

/// Computes the label given the key len
pub fn compute_label<T: Extend<u8>>(len: usize, label: &[u8], out: &mut T) {
    const TLS_LABEL: &[u8] = b"tls13 ";
    let label_len = TLS_LABEL.len() + label.len();
    debug_assert!(label_len <= u8::MAX as usize, "label is too long");

    out.extend((len as u16).to_be_bytes().iter().cloned());
    out.extend(Some(label_len as u8));
    out.extend(TLS_LABEL.iter().cloned());
    out.extend(label.iter().cloned());
    out.extend(Some(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_test() {
        assert_eq!(compute_vec_label(32, b"client in"), CLIENT_IN);
        assert_eq!(compute_vec_label(32, b"server in"), SERVER_IN);
    }

    #[test]
    fn len_16_test() {
        assert_eq!(compute_vec_label(16, b"quic key"), QUIC_KEY_16);
        assert_eq!(compute_vec_label(12, b"quic iv"), QUIC_IV_12);
        assert_eq!(compute_vec_label(16, b"quic hp"), QUIC_HP_16);
        assert_eq!(compute_vec_label(16, b"quic ku"), QUIC_KU_16);
    }

    #[test]
    fn len_32_test() {
        assert_eq!(compute_vec_label(32, b"quic key"), QUIC_KEY_32);
        assert_eq!(compute_vec_label(32, b"quic hp"), QUIC_HP_32);
        assert_eq!(compute_vec_label(32, b"quic ku"), QUIC_KU_32);
    }

    #[test]
    fn len_48_test() {
        assert_eq!(compute_vec_label(48, b"quic ku"), QUIC_KU_48);
    }

    fn compute_vec_label(len: usize, label: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        compute_label(len, label, &mut out);
        out
    }
}
