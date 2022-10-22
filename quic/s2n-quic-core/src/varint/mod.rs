// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    convert::{TryFrom, TryInto},
    fmt,
    ops::Deref,
};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

#[cfg(test)]
mod tests;

//= https://www.rfc-editor.org/rfc/rfc9000#section-16
//# QUIC packets and frames commonly use a variable-length encoding for
//# non-negative integer values.  This encoding ensures that smaller
//# integer values need fewer bytes to encode.

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

pub const MAX_VARINT_VALUE: u64 = 4_611_686_018_427_387_903;

#[derive(Debug)]
pub struct VarIntError;

impl fmt::Display for VarIntError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "varint range exceeded")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for VarIntError {}

// https://godbolt.org/z/ToTvPD
#[inline(always)]
fn read_table(x: u64) -> (u64, usize, u64) {
    debug_assert!(x <= MAX_VARINT_VALUE);

    macro_rules! table {
        ($(($two_bit:expr, $length:expr, $usable_bits:expr, $max_value:expr);)*) => {{
            let mut two_bit = 0;
            let leading_zeros = x.leading_zeros();
            $(
                two_bit += if leading_zeros < (64 - $usable_bits) {
                    1
                } else {
                    0
                };
            )*

            let len = 1 << two_bit;
            let usable_bits = len * 8 - 2;

            debug_assert_eq!(len as usize, encoding_size(x));

            (two_bit, len as usize, usable_bits)
        }};
    }

    table! {
        (0b00, 1, 6 , 63);
        (0b01, 2, 14, 16_383);
        (0b10, 4, 30, 1_073_741_823);
    }
}

#[inline(always)]
fn encoding_size(x: u64) -> usize {
    debug_assert!(x <= MAX_VARINT_VALUE);

    macro_rules! table {
        ($(($two_bit:expr, $length:expr, $usable_bits:expr, $max_value:expr);)*) => {{
            let leading_zeros = x.leading_zeros();
            let mut len = 1;

            $(
                if leading_zeros < (64 - $usable_bits) {
                    len = $length * 2;
                };
            )*

            len
        }};
    }

    table! {
        (0b00, 1, 6 , 63);
        (0b01, 2, 14, 16_383);
        (0b10, 4, 30, 1_073_741_823);
    }
}

// === API ===

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
pub struct VarInt(#[cfg_attr(any(feature = "generator", test), generator(Self::GENERATOR))] u64);

impl fmt::Display for VarInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl VarInt {
    pub const MAX: Self = Self(MAX_VARINT_VALUE);

    pub const ZERO: Self = Self(0);

    #[cfg(any(feature = "generator", test))]
    const GENERATOR: core::ops::RangeInclusive<u64> = 0..=MAX_VARINT_VALUE;

    pub fn new(v: u64) -> Result<Self, VarIntError> {
        if v > MAX_VARINT_VALUE {
            return Err(VarIntError);
        }
        Ok(Self(v))
    }

    /// Returns a `VarInt` without validating the value is less than VarInt::MAX
    ///
    /// # Safety
    ///
    /// Callers need to ensure the value is less than or equal to VarInt::MAX
    pub const unsafe fn new_unchecked(value: u64) -> Self {
        Self(value)
    }

    pub const fn from_u8(v: u8) -> Self {
        Self(v as u64)
    }

    pub const fn from_u16(v: u16) -> Self {
        Self(v as u64)
    }

    pub const fn from_u32(v: u32) -> Self {
        Self(v as u64)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn checked_add(self, value: Self) -> Option<Self> {
        Self::new(self.0.checked_add(value.0)?).ok()
    }

    #[inline]
    pub fn checked_add_usize(self, value: usize) -> Option<Self> {
        let value = value.try_into().ok()?;
        self.checked_add(value)
    }

    #[inline]
    #[must_use]
    pub fn saturating_add(self, value: Self) -> Self {
        Self::new(self.0.saturating_add(value.0)).unwrap_or(Self::MAX)
    }

    #[inline]
    pub fn checked_sub(self, value: Self) -> Option<Self> {
        Some(Self(self.0.checked_sub(value.0)?))
    }

    #[inline]
    #[must_use]
    pub fn saturating_sub(self, value: Self) -> Self {
        Self(self.0.saturating_sub(value.0))
    }

    #[inline]
    pub fn checked_mul(self, value: Self) -> Option<Self> {
        Self::new(self.0.checked_mul(value.0)?).ok()
    }

    #[inline]
    #[must_use]
    pub fn saturating_mul(self, value: Self) -> Self {
        Self::new(self.0.saturating_mul(value.0)).unwrap_or(Self::MAX)
    }

    #[inline]
    pub fn checked_div(self, value: Self) -> Option<Self> {
        Some(Self(self.0.checked_div(value.0)?))
    }

    /// Re-encodes a replacement value where `self` was used as a placeholder.
    #[inline]
    pub fn encode_updated<E: Encoder>(self, replacement: Self, encoder: &mut E) {
        debug_assert!(
            self.encoding_table_entry().1 >= replacement.encoding_table_entry().1,
            "the replacement encoding_size should not be greater than the previous value"
        );

        replacement.encode_with_table_entry(self.encoding_table_entry(), encoder);
    }

    #[inline]
    fn encode_with_table_entry<E: Encoder>(
        self,
        (two_bit, len, usable_bits): (u64, usize, u64),
        encoder: &mut E,
    ) {
        encoder.write_sized(len, |buffer| {
            let bytes = (two_bit << usable_bits | self.0).to_be_bytes();

            unsafe {
                // Safety: the encoder will have checked the buffer size
                //         before passing it so we don't need to pay the bounds
                //         check cost twice.

                // This code looks a little scary so here's some comments describing what
                // is happening.

                // We bitwise-and the two bit value to ensure the compiler can prove the
                // `unreachable` is actually unreachable.
                match two_bit & 0b11 {
                    0b00 => {
                        // If the two bit value is 0b00 it means we only have 1 byte to
                        // encode so we copy the last byte from our big endian encoded value
                        // into the first byte of the buffer
                        debug_assert_eq!(buffer.len(), 1);
                        *buffer.get_unchecked_mut(0) = *bytes.get_unchecked(7);
                    }
                    0b01 => {
                        // If the two bit value is 0b01 it means we have a 2 byte value to
                        // encode so we copy the last 2 bytes from our big endian encoded value
                        // into the first 2 bytes of the buffer
                        debug_assert_eq!(buffer.len(), 2);
                        buffer
                            .get_unchecked_mut(..2)
                            .copy_from_slice(bytes.get_unchecked(6..));
                    }
                    0b10 => {
                        // If the two bit value is 0b10 it means we have a 4 byte value to
                        // encode so we copy the last 4 bytes from our big endian encoded value
                        // into the first 4 bytes of the buffer
                        debug_assert_eq!(buffer.len(), 4);
                        buffer
                            .get_unchecked_mut(..4)
                            .copy_from_slice(bytes.get_unchecked(4..));
                    }
                    0b11 => {
                        // If the two bit value is 0b11 it means we have a 8 byte value to
                        // encode so we copy all of the bytes into the buffer
                        debug_assert_eq!(buffer.len(), 8);
                        buffer
                            .get_unchecked_mut(..8)
                            .copy_from_slice(bytes.get_unchecked(..8));
                    }
                    _ => unreachable!(),
                }
            }
        })
    }

    #[inline]
    fn encoding_table_entry(self) -> (u64, usize, u64) {
        read_table(self.0)
    }
}

impl EncoderValue for VarInt {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.encode_with_table_entry(self.encoding_table_entry(), encoder);
    }

    #[inline]
    fn encoding_size(&self) -> usize {
        encoding_size(self.0)
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        encoding_size(self.0)
    }
}

decoder_value!(
    impl<'a> VarInt {
        fn decode(buffer: Buffer) -> Result<Self> {
            let header = buffer.peek_byte(0)?;

            Ok(match (header >> 6) & 0b11 {
                0b00 => {
                    let value = header & (2u8.pow(6) - 1);
                    let buffer = buffer.skip(1)?;
                    (Self(value.into()), buffer)
                }
                0b01 => {
                    let (value, buffer) = buffer.decode::<u16>()?;
                    let value = value & (2u16.pow(14) - 1);
                    (Self(value.into()), buffer)
                }
                0b10 => {
                    let (value, buffer) = buffer.decode::<u32>()?;
                    let value = value & (2u32.pow(30) - 1);
                    (Self(value.into()), buffer)
                }
                0b11 => {
                    let (value, buffer) = buffer.decode::<u64>()?;
                    let value = value & (2u64.pow(62) - 1);
                    (Self(value), buffer)
                }
                _ => unreachable!(),
            })
        }
    }
);

#[cfg(test)]
mod encoder_tests {
    use super::*;
    use core::mem::size_of;
    use s2n_codec::{DecoderBuffer, EncoderBuffer};

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
}

impl AsRef<u64> for VarInt {
    #[inline]
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl Deref for VarInt {
    type Target = u64;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

macro_rules! impl_from_lesser {
    ($ty:ty) => {
        impl From<$ty> for VarInt {
            #[inline]
            fn from(value: $ty) -> Self {
                Self(value.into())
            }
        }
    };
}

impl_from_lesser!(u8);
impl_from_lesser!(u16);
impl_from_lesser!(u32);

impl From<VarInt> for u64 {
    #[inline]
    fn from(v: VarInt) -> u64 {
        v.0
    }
}

impl TryFrom<usize> for VarInt {
    type Error = VarIntError;

    #[inline]
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value as u64)
    }
}

impl TryInto<usize> for VarInt {
    type Error = <usize as TryFrom<u64>>::Error;

    #[inline]
    fn try_into(self) -> Result<usize, Self::Error> {
        self.0.try_into()
    }
}

impl TryFrom<u64> for VarInt {
    type Error = VarIntError;

    #[inline]
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<u128> for VarInt {
    type Error = VarIntError;

    #[inline]
    fn try_from(value: u128) -> Result<Self, Self::Error> {
        if value > MAX_VARINT_VALUE as u128 {
            Err(VarIntError)
        } else {
            Ok(Self(value as u64))
        }
    }
}

impl core::ops::Add for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn add(self, rhs: Self) -> Self {
        if cfg!(debug_assertions) {
            self.checked_add(rhs).expect("VarInt overflow occurred")
        } else {
            Self(self.0 + rhs.0)
        }
    }
}

impl core::ops::Add<usize> for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn add(self, rhs: usize) -> Self {
        if cfg!(debug_assertions) {
            self.checked_add(VarInt::new(rhs as u64).expect("VarInt overflow occurred"))
                .expect("VarInt overflow occurred")
        } else {
            Self(self.0 + rhs as u64)
        }
    }
}

impl core::ops::AddAssign<Self> for VarInt {
    #[inline]
    #[track_caller]
    fn add_assign(&mut self, rhs: Self) {
        if cfg!(debug_assertions) {
            *self = self.checked_add(rhs).expect("VarInt overflow occurred")
        } else {
            self.0 += rhs.0
        }
    }
}

impl core::ops::AddAssign<usize> for VarInt {
    #[inline]
    #[track_caller]
    fn add_assign(&mut self, rhs: usize) {
        if cfg!(debug_assertions) {
            *self = self
                .checked_add(VarInt::new(rhs as u64).expect("VarInt overflow occurred"))
                .expect("VarInt overflow occurred")
        } else {
            self.0 += rhs as u64
        }
    }
}

impl core::ops::Sub for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn sub(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 - rhs.0)
    }
}

impl core::ops::Sub<usize> for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn sub(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 - rhs as u64)
    }
}

impl core::ops::SubAssign<Self> for VarInt {
    #[inline]
    #[track_caller]
    fn sub_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 -= rhs.0
    }
}

impl core::ops::SubAssign<usize> for VarInt {
    #[inline]
    #[track_caller]
    fn sub_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 -= rhs as u64
    }
}

impl core::ops::Mul for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn mul(self, rhs: Self) -> Self {
        if cfg!(debug_assertions) {
            self.checked_mul(rhs).expect("VarInt overflow occurred")
        } else {
            Self(self.0 * rhs.0)
        }
    }
}

impl core::ops::Mul<usize> for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn mul(self, rhs: usize) -> Self {
        if cfg!(debug_assertions) {
            self.checked_mul(VarInt::new(rhs as u64).expect("VarInt overflow occurred"))
                .expect("VarInt overflow occurred")
        } else {
            Self(self.0 * rhs as u64)
        }
    }
}

impl core::ops::MulAssign<Self> for VarInt {
    #[inline]
    #[track_caller]
    fn mul_assign(&mut self, rhs: Self) {
        if cfg!(debug_assertions) {
            *self = self.checked_mul(rhs).expect("VarInt overflow occurred")
        } else {
            self.0 *= rhs.0
        }
    }
}

impl core::ops::MulAssign<usize> for VarInt {
    #[inline]
    #[track_caller]
    fn mul_assign(&mut self, rhs: usize) {
        if cfg!(debug_assertions) {
            *self = self
                .checked_mul(VarInt::new(rhs as u64).expect("VarInt overflow occurred"))
                .expect("VarInt overflow occurred")
        } else {
            self.0 *= rhs as u64
        }
    }
}

impl core::ops::Div for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn div(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 / rhs.0)
    }
}

impl core::ops::Div<usize> for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn div(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 / rhs as u64)
    }
}

impl core::ops::DivAssign<Self> for VarInt {
    #[inline]
    #[track_caller]
    fn div_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 /= rhs.0
    }
}

impl core::ops::DivAssign<usize> for VarInt {
    #[inline]
    #[track_caller]
    fn div_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 /= rhs as u64
    }
}

impl core::ops::Rem for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn rem(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0.rem(rhs.0))
    }
}

impl core::ops::Rem<usize> for VarInt {
    type Output = Self;

    #[inline]
    #[track_caller]
    fn rem(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0.rem(rhs as u64))
    }
}

impl core::ops::RemAssign<Self> for VarInt {
    #[inline]
    #[track_caller]
    fn rem_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 %= rhs.0
    }
}

impl core::ops::RemAssign<usize> for VarInt {
    #[inline]
    #[track_caller]
    fn rem_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 %= rhs as u64
    }
}

impl PartialEq<u64> for VarInt {
    #[inline]
    fn eq(&self, other: &u64) -> bool {
        self.0.eq(other)
    }
}

impl PartialEq<usize> for VarInt {
    #[inline]
    fn eq(&self, other: &usize) -> bool {
        self.0.eq(&(*other as u64))
    }
}

impl PartialOrd<u64> for VarInt {
    #[inline]
    fn partial_cmp(&self, other: &u64) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialOrd<usize> for VarInt {
    #[inline]
    fn partial_cmp(&self, other: &usize) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&(*other as u64))
    }
}
