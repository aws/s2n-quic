// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::varint::VarInt;
use bolero::check;
use s2n_codec::assert_codec_round_trip_bytes;

#[test]
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
