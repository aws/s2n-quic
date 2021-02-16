use core::{
    convert::{TryFrom, TryInto},
    ops::Deref,
};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

#[cfg(feature = "generator")]
use bolero_generator::*;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#16
//# QUIC packets and frames commonly use a variable-length encoding for
//# non-negative integer values.  This encoding ensures that smaller
//# integer values need fewer bytes to encode.

//# The QUIC variable-length integer encoding reserves the two most
//# significant bits of the first byte to encode the base 2 logarithm of
//# the integer encoding length in bytes.  The integer value is encoded
//# on the remaining bits, in network byte order.

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#16
//# This means that integers are encoded on 1, 2, 4, or 8 bytes and can
//# encode 6, 14, 30, or 62 bit values respectively.  Table 4 summarizes
//# the encoding properties.
//#
//#        +======+========+=============+=======================+
//#        | 2Bit | Length | Usable Bits | Range                 |
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

// https://godbolt.org/z/ToTvPD
#[inline]
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

            (two_bit, len as usize, usable_bits)
        }};
    }

    table! {
        (0b00, 1, 6 , 63);
        (0b01, 2, 14, 16_383);
        (0b10, 4, 30, 1_073_741_823);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
    fn table_snapshot_test() {
        use insta::assert_debug_snapshot;
        assert_debug_snapshot!("max_value", MAX_VARINT_VALUE);

        // These values are derived from the "usable bits" column in the table: V and V-1
        for i in [0, 1, 5, 6, 13, 14, 29, 30, 61].iter().cloned() {
            assert_debug_snapshot!(format!("table_2_pow_{}_", i), read_table(2u64.pow(i)));
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#16
    //# For example, the eight byte sequence c2 19 7c 5e ff 14 e8 8c (in
    //# hexadecimal) decodes to the decimal value 151288809941952652; the
    //# four byte sequence 9d 7f 3e 7d decodes to 494878333; the two byte
    //# sequence 7b bd decodes to 15293; and the single byte 25 decodes to 37
    //# (as does the two byte sequence 40 25).

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
}

// === API ===

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[cfg_attr(feature = "generator", derive(TypeGenerator))]
pub struct VarInt(#[cfg_attr(feature = "generator", generator(0..=MAX_VARINT_VALUE))] u64);

impl core::fmt::Display for VarInt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl VarInt {
    pub const MAX: Self = Self(MAX_VARINT_VALUE);

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
    pub fn saturating_add(self, value: Self) -> Self {
        Self::new(self.0.saturating_add(value.0)).unwrap_or(Self::MAX)
    }

    #[inline]
    pub fn checked_sub(self, value: Self) -> Option<Self> {
        Some(Self(self.0.checked_sub(value.0)?))
    }

    #[inline]
    pub fn saturating_sub(self, value: Self) -> Self {
        Self(self.0.saturating_sub(value.0))
    }

    #[inline]
    pub fn checked_mul(self, value: Self) -> Option<Self> {
        Self::new(self.0.checked_mul(value.0)?).ok()
    }

    #[inline]
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
                match two_bit & 0b11 {
                    0b00 => {
                        debug_assert_eq!(buffer.len(), 1);
                        *buffer.get_unchecked_mut(0) = *bytes.get_unchecked(7);
                    }
                    0b01 => {
                        debug_assert_eq!(buffer.len(), 2);
                        buffer
                            .get_unchecked_mut(..2)
                            .copy_from_slice(bytes.get_unchecked(6..));
                    }
                    0b10 => {
                        debug_assert_eq!(buffer.len(), 4);
                        buffer
                            .get_unchecked_mut(..4)
                            .copy_from_slice(bytes.get_unchecked(4..));
                    }
                    0b11 => {
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
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl Deref for VarInt {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

macro_rules! impl_from_lesser {
    ($ty:ty) => {
        impl From<$ty> for VarInt {
            fn from(value: $ty) -> Self {
                Self(value.into())
            }
        }
    };
}

impl_from_lesser!(u8);
impl_from_lesser!(u16);
impl_from_lesser!(u32);

impl Into<u64> for VarInt {
    fn into(self) -> u64 {
        self.0
    }
}

impl TryFrom<usize> for VarInt {
    type Error = VarIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value as u64)
    }
}

impl TryInto<usize> for VarInt {
    type Error = <usize as TryFrom<u64>>::Error;

    fn try_into(self) -> Result<usize, Self::Error> {
        self.0.try_into()
    }
}

impl TryFrom<u64> for VarInt {
    type Error = VarIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<u128> for VarInt {
    type Error = VarIntError;

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
    fn add(self, rhs: usize) -> Self {
        if cfg!(debug_assertions) {
            self.checked_add(VarInt::new(rhs as u64).expect("VarInt overflow occured"))
                .expect("VarInt overflow occurred")
        } else {
            Self(self.0 + rhs as u64)
        }
    }
}

impl core::ops::AddAssign<Self> for VarInt {
    #[inline]
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
    fn add_assign(&mut self, rhs: usize) {
        if cfg!(debug_assertions) {
            *self = self
                .checked_add(VarInt::new(rhs as u64).expect("VarInt overflow occured"))
                .expect("VarInt overflow occurred")
        } else {
            self.0 += rhs as u64
        }
    }
}

impl core::ops::Sub for VarInt {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 - rhs.0)
    }
}

impl core::ops::Sub<usize> for VarInt {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 - rhs as u64)
    }
}

impl core::ops::SubAssign<Self> for VarInt {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 -= rhs.0
    }
}

impl core::ops::SubAssign<usize> for VarInt {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 -= rhs as u64
    }
}

impl core::ops::Mul for VarInt {
    type Output = Self;

    #[inline]
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
    fn mul(self, rhs: usize) -> Self {
        if cfg!(debug_assertions) {
            self.checked_mul(VarInt::new(rhs as u64).expect("VarInt overflow occured"))
                .expect("VarInt overflow occurred")
        } else {
            Self(self.0 * rhs as u64)
        }
    }
}

impl core::ops::MulAssign<Self> for VarInt {
    #[inline]
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
    fn mul_assign(&mut self, rhs: usize) {
        if cfg!(debug_assertions) {
            *self = self
                .checked_mul(VarInt::new(rhs as u64).expect("VarInt overflow occured"))
                .expect("VarInt overflow occurred")
        } else {
            self.0 *= rhs as u64
        }
    }
}

impl core::ops::Div for VarInt {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 / rhs.0)
    }
}

impl core::ops::Div<usize> for VarInt {
    type Output = Self;

    #[inline]
    fn div(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0 / rhs as u64)
    }
}

impl core::ops::DivAssign<Self> for VarInt {
    #[inline]
    fn div_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 /= rhs.0
    }
}

impl core::ops::DivAssign<usize> for VarInt {
    #[inline]
    fn div_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 /= rhs as u64
    }
}

impl core::ops::Rem for VarInt {
    type Output = Self;

    #[inline]
    fn rem(self, rhs: Self) -> Self {
        // Bounds check is inherited from u64
        Self(self.0.rem(rhs.0))
    }
}

impl core::ops::Rem<usize> for VarInt {
    type Output = Self;

    #[inline]
    fn rem(self, rhs: usize) -> Self {
        // Bounds check is inherited from u64
        Self(self.0.rem(rhs as u64))
    }
}

impl core::ops::RemAssign<Self> for VarInt {
    #[inline]
    fn rem_assign(&mut self, rhs: Self) {
        // Bounds check is inherited from u64
        self.0 %= rhs.0
    }
}

impl core::ops::RemAssign<usize> for VarInt {
    #[inline]
    fn rem_assign(&mut self, rhs: usize) {
        // Bounds check is inherited from u64
        self.0 %= rhs as u64
    }
}

impl PartialEq<u64> for VarInt {
    fn eq(&self, other: &u64) -> bool {
        self.0.eq(other)
    }
}

impl PartialEq<usize> for VarInt {
    fn eq(&self, other: &usize) -> bool {
        self.0.eq(&(*other as u64))
    }
}

impl PartialOrd<u64> for VarInt {
    fn partial_cmp(&self, other: &u64) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialOrd<usize> for VarInt {
    fn partial_cmp(&self, other: &usize) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&(*other as u64))
    }
}
