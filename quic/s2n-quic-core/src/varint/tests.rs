// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::varint::VarInt;
use bolero::check;
use s2n_codec::assert_codec_round_trip_bytes;

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn round_trip() {
    check!().for_each(|input| {
        for value in assert_codec_round_trip_bytes!(VarInt, input) {
            let _ = value.checked_add(value);
            let _ = value.checked_sub(value);
            let _ = value.checked_mul(value);
            let _ = value.checked_div(value);
            let _ = value.saturating_add(value);
            let _ = value.saturating_sub(value);
            let _ = value.saturating_mul(value);
        }
    });
}

#[test]
#[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
fn table_snapshot_test() {
    use insta::assert_debug_snapshot;
    assert_debug_snapshot!("max_value", MAX_VARINT_VALUE);

    // These values are derived from the "usable bits" column in the table: V and V-1
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61] {
        assert_debug_snapshot!(format!("table_2_pow_{}_", i), read_table(2u64.pow(i)));
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-A.1
//# For example, the eight-byte sequence 0xc2197c5eff14e88c decodes to
//# the decimal value 151,288,809,941,952,652; the four-byte sequence
//# 0x9d7f3e7d decodes to 494,878,333; the two-byte sequence 0x7bbd
//# decodes to 15,293; and the single byte 0x25 decodes to 37 (as does
//# the two-byte sequence 0x4025).

macro_rules! sequence_test {
    ($name:ident($input:expr, $expected:expr)) => {
        #[test]
        fn $name() {
            use s2n_codec::assert_codec_round_trip_value;

            let input = $input;
            let expected = VarInt::new($expected).unwrap();
            let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
            assert_eq!(&input[..], &actual_bytes[..]);
        }
    };
}

sequence_test!(eight_byte_sequence_test(
    [0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c],
    151_288_809_941_952_652
));

sequence_test!(four_byte_sequence_test(
    [0x9d, 0x7f, 0x3e, 0x7d],
    494_878_333
));

sequence_test!(two_byte_sequence_test([0x7b, 0xbd], 15293));

sequence_test!(one_byte_sequence_test([0x25], 37));

mod tests {
    use super::*;
    use s2n_codec::assert_codec_round_trip_value;

    #[cfg(kani)]
    #[kani::proof]
    fn kani_one_byte_sequence_test() {
        // https://www.rfc-editor.org/rfc/rfc9000#section-16
        // One byte sequences have the first two MSBs encoded as 00; the
        // last six bits are the usable values.
        let first_byte: u8 = kani::any::<u8>() & 0x3f;
        let byte_sequence = [first_byte];
        let expected = VarInt::new(first_byte as u64).unwrap();
        let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
        assert_eq!(&byte_sequence[..], &actual_bytes[..]);
    }

    #[cfg(kani)]
    #[kani::proof]
    fn kani_two_byte_sequence_test() {
        // https://www.rfc-editor.org/rfc/rfc9000#section-16
        // Two byte sequences have the first two MSBs encoded as 01; the
        // last 14 bits are the usable values.
        let first_byte: u8 = (kani::any::<u8>() & 0x2f) | 0x40;
        let second_byte: u8 = kani::any();
        // The s2n-quic implementation always chooses the smallest encoding possible.
        // This means if we wish to test two-byte sequences, we need to encode a number
        // that is > 63.
        kani::assume(second_byte > 63);
        let byte_sequence = [first_byte, second_byte];
        let expected_val: u64 = (((first_byte & 0x3f) as u64) << 8) | (second_byte as u64);
        assert!(expected_val <= 16383);
        let expected = VarInt::new(expected_val).unwrap();
        let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
        assert_eq!(&byte_sequence[..], &actual_bytes[..]);
    }

    #[cfg(kani)]
    #[kani::proof]
    fn kani_four_byte_sequence_test() {
        // https://www.rfc-editor.org/rfc/rfc9000#section-16
        // Four byte sequences have the first two MSBs encoded as 10; the
        // last 30 bits are the usable values.
        let first_byte: u8 = (kani::any::<u8>() & 0x3f) | 0x80;
        let second_byte: u8 = kani::any();
        // The s2n-quic implementation always chooses the smallest encoding possible.
        // This means if we wish to test the four-byte sequences, we need to encode a number
        // that is > 16383 or 0b0011 1111 1111 1111.
        let third_byte: u8 = kani::any();
        kani::assume(third_byte > 0x3f);
        let byte_sequence = [first_byte, second_byte, third_byte, 0xff];
        let expected_val: u64 = (((first_byte & 0x3f) as u64) << 24) 
            | (second_byte as u64) << 16 | (third_byte as u64) << 8 | 0xff;
        assert!(expected_val <= 1073741823);
        let expected = VarInt::new(expected_val).unwrap();
        let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
        assert_eq!(&byte_sequence[..], &actual_bytes[..]);
    }

    #[cfg(kani)]
    #[kani::proof]
    fn kani_eight_byte_sequence_test() {
        // https://www.rfc-editor.org/rfc/rfc9000#section-16
        // Eight byte sequences have the first two MSBs encoded as 11; the
        // last 62 bits are the usable values.
        let first_byte: u8 = (kani::any::<u8>() & 0x3f) | 0xc0;
        let second_byte: u8 = kani::any();
        // The s2n-quic implementation always chooses the smallest encoding possible.
        // This means if we wish to test eight-byte sequences, we need to encode a number
        // that is > 1073741823 or 0b0011 1111 1111 1111 1111 1111 1111 1111
        let third_byte: u8 = kani::any();
        let fourth_byte: u8 = kani::any();
        kani::assume(fourth_byte > 0x3f);
        let byte_sequence = [first_byte, second_byte, third_byte, fourth_byte, 0xff, 0xff, 0xff, 0xff];
        let expected_val: u64 = (((first_byte & 0x3f) as u64) << 56) 
            | (second_byte as u64) << 48 | (third_byte as u64) << 40 | (fourth_byte as u64) << 32 
            | (0xff << 24) | (0xff << 16) | (0xff << 8) | 0xff;
        assert!(expected_val <= 4611686018427387903);
        let expected = VarInt::new(expected_val).unwrap();
        let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
        assert_eq!(&byte_sequence[..], &actual_bytes[..]);
    }
}
