// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::MAX_VARINT_VALUE;
use core::fmt;
use s2n_codec::{Encoder, EncoderValue};

//# The QUIC variable-length integer encoding reserves the two most
//# significant bits of the first byte to encode the base 2 logarithm of
//# the integer encoding length in bytes.  The integer value is encoded
//# on the remaining bits, in network byte order.

//= https://www.rfc-editor.org/rfc/rfc9000#section-16
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

macro_rules! call_table {
    ($table:ident) => {
        $table! {
            (0b10, 4, 30, 1_073_741_823);
            (0b01, 2, 14, 16_383);
            (0b00, 1, 6 , 63);
        }
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The two bits to use for this entry, encoding in native-endian
    pub two_bit: u64,
    /// The two bits to use for this entry, encoding in big-endian
    pub two_bit_be: u64,
    /// The number of bytes required to encode this entry
    pub len: usize,
    /// The number of usable bits for the actual value
    pub usable_bits: u64,
    /// The number of bits to shift for encoding/decoding
    pub shift: u64,
}

impl fmt::Debug for Entry {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Entry")
            .field("two_bit", &self.two_bit)
            .field("two_bit_be", &self.two_bit_be.to_ne_bytes())
            .field("len", &self.len)
            .field("usable_bits", &self.usable_bits)
            .field("shift", &self.shift)
            .finish()
    }
}

impl Entry {
    /// Returns the entry in the table corresponding to the provided value
    #[inline(always)]
    pub fn read(x: u64) -> Self {
        let optimized = Self::read_optimized(x);
        debug_assert_eq!(optimized, Self::read_rfc(x));
        optimized
    }

    /// A simple implementation of the table lookup, mostly based on the RFC table
    #[inline(always)]
    pub fn read_rfc(x: u64) -> Self {
        // write the table from the RFC in a non-optimized format and make sure the
        // actual results match
        #[allow(clippy::match_overlapping_arm)]
        match x {
            0..=63 => Self {
                two_bit: 0b00,
                two_bit_be: 0b00,
                len: 1,
                usable_bits: 6,
                shift: 56,
            },
            0..=16383 => Self {
                two_bit: 0b01,
                two_bit_be: (0b01u64 << 62).to_be(),
                len: 2,
                usable_bits: 14,
                shift: 48,
            },
            0..=1073741823 => Self {
                two_bit: 0b10,
                two_bit_be: (0b10u64 << 62).to_be(),
                len: 4,
                usable_bits: 30,
                shift: 32,
            },
            0..=4611686018427387903 => Self {
                two_bit: 0b11,
                two_bit_be: (0b11u64 << 62).to_be(),
                len: 8,
                usable_bits: 62,
                shift: 0,
            },
            _ => unreachable!(),
        }
    }

    // https://godbolt.org/z/9xrxd1osd
    #[inline(always)]
    pub fn read_optimized(x: u64) -> Self {
        unsafe {
            crate::assume!(x <= MAX_VARINT_VALUE);
        }

        macro_rules! table {
            ($(($two_bit:expr, $length:expr, $usable_bits:expr, $max_value:expr);)*) => {{
                let mut shift = 0;
                let mut usable_bits = 62;
                let mut two_bit = 0b11u64;
                let mut two_bit_be = (two_bit << 62).to_be();
                let mut len = 8;

                $(
                    if x <= $max_value {
                        shift = 62 - $usable_bits;
                        usable_bits = $usable_bits;
                        two_bit -= 1u64;
                        two_bit_be -= (1u64 << 62).to_be();
                        len = $length;
                    }
                )*

                Self { two_bit, two_bit_be, len, usable_bits, shift }
            }};
        }

        call_table!(table)
    }

    #[inline(always)]
    pub fn format(&self, value: u64) -> Formatted {
        let encoded_be = (value << self.shift).to_be() | self.two_bit_be;
        let len = self.len;
        Formatted { encoded_be, len }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formatted {
    pub(crate) encoded_be: u64,
    pub(crate) len: usize,
}

impl Formatted {
    #[inline(always)]
    pub(super) fn new(x: u64) -> Self {
        unsafe {
            crate::assume!(x <= MAX_VARINT_VALUE);
        }

        macro_rules! table {
            ($(($two_bit:expr, $length:expr, $usable_bits:expr, $max_value:expr);)*) => {{
                let mut shift = 0;
                let mut two_bit = (0b11u64 << 62).to_be();
                let mut len = 8;

                $(
                    if x <= $max_value {
                        shift = 62 - $usable_bits;
                        two_bit -= (1u64 << 62).to_be();
                        len = $length;
                    }
                )*

                #[cfg(debug_assertions)]
                {
                    // make sure we end up with the same computed entry
                    let entry = Entry::read_rfc(x);
                    assert_eq!(entry.shift, shift);
                    assert_eq!(entry.two_bit_be, two_bit);
                    assert_eq!(entry.len, len);
                }

                let encoded_be = (x << shift).to_be() | two_bit;

                let v = Self { encoded_be, len };

                v.invariants();

                v
            }};
        }

        call_table!(table)
    }

    #[inline(always)]
    pub(super) fn encode_oversized<E: Encoder>(&self, encoder: &mut E) {
        debug_assert!(encoder.remaining_capacity() >= 8);
        self.invariants();

        encoder.write_sized(self.len, |dest| {
            let dest = dest.as_mut_ptr() as *mut [u8; 8];
            let bytes = self.encoded_be.to_ne_bytes();
            unsafe {
                core::ptr::write(dest, bytes);
            }
        });
    }

    #[inline(always)]
    pub(super) fn encode_maybe_undersized<E: Encoder>(&self, encoder: &mut E) {
        self.invariants();

        let len = self.len;

        encoder.write_sized(len, |dst| {
            let src = self.encoded_be.to_ne_bytes();
            unsafe {
                use core::ptr::copy_nonoverlapping as copy;

                crate::assume!(dst.len() == len);

                let src = src.as_ptr();
                let dst = dst.as_mut_ptr();

                match len {
                    1 => copy(src, dst, 1),
                    2 => copy(src, dst, 2),
                    4 => copy(src, dst, 4),
                    8 => copy(src, dst, 8),
                    _ => {
                        assume!(false, "invalid len: {len}");
                    }
                }
            }
        });
    }

    #[inline(always)]
    pub(super) fn invariants(&self) {
        unsafe {
            let len = self.len;
            // avoid using `contains` since kani wants an unwind for that
            assume!(len == 1 || len == 2 || len == 4 || len == 8);
        }
    }
}

impl EncoderValue for Formatted {
    #[inline(always)]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.invariants();

        // optimize for the case where we have at least 8 bytes left and just write a full u64 to
        // the buffer, but incrementing the offset by `self.len`.
        //
        // ignore this optimization under miri, since we're technically reading beyond the slice
        // that the encoder gives us, which miri complains about.
        if encoder.remaining_capacity() >= 8 && !cfg!(miri) {
            self.encode_oversized(encoder);
        } else {
            self.encode_maybe_undersized(encoder);
        }
    }

    #[inline(always)]
    fn encoding_size(&self) -> usize {
        self.len
    }

    #[inline(always)]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        self.len
    }
}
