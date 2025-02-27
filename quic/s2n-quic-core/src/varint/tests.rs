// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::varint::VarInt;
use bolero::check;
use core::mem::size_of;
use s2n_codec::{
    assert_codec_round_trip_bytes, assert_codec_round_trip_value, DecoderBuffer, EncoderBuffer,
};

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn round_trip_bytes_test() {
    check!().for_each(|input| {
        assert_codec_round_trip_bytes!(VarInt, input);
    });
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(10), kani::solver(cadical))]
fn round_trip_values_test() {
    check!().with_type().cloned().for_each(|v| {
        if let Ok(v) = VarInt::new(v) {
            assert_codec_round_trip_value!(VarInt, v);
        } else {
            assert!(v > MAX_VARINT_VALUE);
        }
    })
}

#[test]
#[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
fn table_snapshot_test() {
    use insta::assert_debug_snapshot;
    assert_debug_snapshot!("max_value", MAX_VARINT_VALUE);

    // These values are derived from the "usable bits" column in the table: V and V-1
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61] {
        assert_debug_snapshot!(
            format!("table_2_pow_{}_", i),
            table::Entry::read(2u64.pow(i))
        );
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.1
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

sequence_test!(eight_byte_sequence_example(
    [0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c],
    151_288_809_941_952_652
));

sequence_test!(four_byte_sequence_example(
    [0x9d, 0x7f, 0x3e, 0x7d],
    494_878_333
));

sequence_test!(two_byte_sequence_example([0x7b, 0xbd], 15293));

sequence_test!(one_byte_sequence_example([0x25], 37));

//= https://www.rfc-editor.org/rfc/rfc9000#section-16
//= type=test
// # +======+========+=============+=======================+
// # | 2MSB | Length | Usable Bits | Range                 |
// # +======+========+=============+=======================+
// # | 00   | 1      | 6           | 0-63                  |
// # +------+--------+-------------+-----------------------+
// # | 01   | 2      | 14          | 0-16383               |
// # +------+--------+-------------+-----------------------+
// # | 10   | 4      | 30          | 0-1073741823          |
// # +------+--------+-------------+-----------------------+
// # | 11   | 8      | 62          | 0-4611686018427387903 |
// # +------+--------+-------------+-----------------------+
#[test]
#[cfg_attr(miri, ignore)]
#[cfg_attr(kani, kani::proof, kani::unwind(3), kani::solver(cadical))]
fn one_byte_sequence_test() {
    bolero::check!()
        .with_type()
        .cloned()
        .for_each(|mut first_byte: u8| {
            first_byte &= 0x3f;
            let byte_sequence = [first_byte];
            let expected = VarInt::new(first_byte as u64).unwrap();
            let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
            assert_eq!(&byte_sequence[..], &actual_bytes[..]);
        });
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg_attr(kani, kani::proof, kani::unwind(4), kani::solver(cadical))]
fn two_byte_sequence_test() {
    // the s2n-quic implementation always chooses the smallest encoding possible.
    // this means if we wish to test two-byte sequences, we need to encode a number
    // that is > 63.
    let g = produce::<(u8, u8)>().filter_gen(|(_, b)| *b > 63);

    bolero::check!()
        .with_generator(g)
        .cloned()
        .for_each(|(mut first_byte, second_byte)| {
            first_byte = (first_byte & 0x3f) | 0x40;
            let byte_sequence = [first_byte, second_byte];
            let expected_val = u16::from_be_bytes([first_byte & 0x3f, second_byte]);
            assert!(expected_val <= 16383);
            let expected = VarInt::new(expected_val as u64).unwrap();
            let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
            assert_eq!(&byte_sequence[..], &actual_bytes[..]);
        });
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg_attr(kani, kani::proof, kani::unwind(10), kani::solver(cadical))]
fn four_byte_sequence_test() {
    // The s2n-quic implementation always chooses the smallest encoding possible.
    // This means if we wish to test the four-byte sequences, we need to encode a number
    // that is > 16383 or 0b0011 1111 1111 1111.
    let g = produce::<(u8, u8, u8)>().filter_gen(|(_, _, c)| *c > 0x3f);

    bolero::check!().with_generator(g).cloned().for_each(
        |(mut first_byte, second_byte, third_byte)| {
            first_byte = (first_byte & 0x3f) | 0x80;
            let byte_sequence = [first_byte, second_byte, third_byte, 0xff];
            let expected_val: u32 =
                u32::from_be_bytes([first_byte & 0x3f, second_byte, third_byte, 0xff]);
            assert!(expected_val <= 1073741823);
            let expected = VarInt::new(expected_val as u64).unwrap();
            let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
            assert_eq!(&byte_sequence[..], &actual_bytes[..]);
        },
    );
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg_attr(kani, kani::proof, kani::unwind(10), kani::solver(cadical))]
fn eight_byte_sequence_test() {
    // The s2n-quic implementation always chooses the smallest encoding possible.
    // This means if we wish to test eight-byte sequences, we need to encode a number
    // that is > 1073741823 or 0b0011 1111 1111 1111 1111 1111 1111 1111
    let g = produce::<(u8, u8, u8, u8)>().filter_gen(|(_, _, _, d)| *d > 0x3f);

    bolero::check!().with_generator(g).cloned().for_each(
        |(mut first_byte, second_byte, third_byte, fourth_byte)| {
            first_byte = (first_byte & 0x3f) | 0xc0;
            let byte_sequence = [
                first_byte,
                second_byte,
                third_byte,
                fourth_byte,
                0xff,
                0xff,
                0xff,
                0xff,
            ];
            let expected_val: u64 = u64::from_be_bytes([
                first_byte & 0x3f,
                second_byte,
                third_byte,
                fourth_byte,
                0xff,
                0xff,
                0xff,
                0xff,
            ]);
            assert!(expected_val <= 4611686018427387903);
            let expected = VarInt::new(expected_val).unwrap();
            let actual_bytes = assert_codec_round_trip_value!(VarInt, expected);
            assert_eq!(&byte_sequence[..], &actual_bytes[..]);
        },
    );
}

fn test_update(initial: VarInt, expected: VarInt, encoder: &mut EncoderBuffer) {
    encoder.set_position(0);
    initial.encode_updated(expected, encoder);
    let decoder = DecoderBuffer::new(encoder.as_mut_slice());
    let (actual, _) = decoder.decode::<VarInt>().unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn encode_updated_test() {
    let mut buffer = [0u8; size_of::<VarInt>()];
    let mut encoder = EncoderBuffer::new(&mut buffer);
    let initial = VarInt::from_u16(1 << 14);
    encoder.encode(&initial);

    test_update(initial, VarInt::from_u32(0), &mut encoder);
    test_update(initial, VarInt::from_u32(1 << 14), &mut encoder);
    test_update(initial, VarInt::from_u32(1 << 29), &mut encoder);
}

#[test]
#[should_panic]
fn encode_updated_invalid_test() {
    let mut buffer = [0u8; size_of::<VarInt>()];
    let mut encoder = EncoderBuffer::new(&mut buffer);
    let initial = VarInt::from_u16(1 << 14);
    encoder.encode(&initial);

    test_update(initial, VarInt::from_u32(1 << 30), &mut encoder);
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
fn table_differential_test() {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-16
    //= type=test
    //# This means that integers are encoded on 1, 2, 4, or 8 bytes and can
    //# encode 6-, 14-, 30-, or 62-bit values, respectively.  Table 4
    //# summarizes the encoding properties.
    //#
    //#        +======+========+=============+=======================+
    //#        | 2MSB | Length | Usable Bits | Range                 |
    //#        +======+========+=============+=======================+
    //#        | 00   | 1      | 6           | 0-63                  |
    //#        +------+--------+-------------+-----------------------+
    //#        | 01   | 2      | 14          | 0-16383               |
    //#        +------+--------+-------------+-----------------------+
    //#        | 10   | 4      | 30          | 0-1073741823          |
    //#        +------+--------+-------------+-----------------------+
    //#        | 11   | 8      | 62          | 0-4611686018427387903 |
    //#        +------+--------+-------------+-----------------------+

    check!()
        .with_generator(0..=MAX_VARINT_VALUE)
        .cloned()
        .for_each(|v| {
            let expected = table::Entry::read_rfc(v);
            let actual = table::Entry::read_optimized(v);
            assert_eq!(actual, expected);
        })
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
fn checked_ops_test() {
    check!().with_type().cloned().for_each(|(a, b)| {
        if let (Ok(a_v), Ok(b_v)) = (VarInt::new(a), VarInt::new(b)) {
            // compares checked operations against the underlying u64 operations
            macro_rules! checked {
                ($checked:ident, $op:tt) => {{
                    if let Some(res) = a_v.$checked(b_v) {
                        // make sure the behavior matches that of u64
                        assert_eq!(Some(res.as_u64()), a.$checked(b));

                        // make sure the normal operation works as well
                        assert_eq!(res, a_v $op b_v);
                    } else {
                        // make sure the behavior matches that of u64
                        let expected = a.$checked(b).and_then(|v| VarInt::new(v).ok());
                        assert!(expected.is_none());
                    }
                }};
            }

            checked!(checked_add, +);
            checked!(checked_sub, -);

            // kani currently spins for a really long time to handle these checks
            #[cfg(any(not(kani), kani_slow))]
            checked!(checked_mul, *);
            #[cfg(any(not(kani), kani_slow))]
            checked!(checked_div, /);

            // compares saturating operations against the underlying u64 operations
            macro_rules! saturating {
                ($name:ident) => {{
                    let actual = a_v.$name(b_v);
                    let expected = VarInt::new(a.$name(b)).unwrap_or(VarInt::MAX);
                    assert_eq!(actual, expected);
                }};
            }

            saturating!(saturating_add);

            // kani currently spins for a really long time to handle these checks
            #[cfg(any(not(kani), kani_slow))]
            saturating!(saturating_sub);
            #[cfg(any(not(kani), kani_slow))]
            saturating!(saturating_mul);
        }
    })
}
