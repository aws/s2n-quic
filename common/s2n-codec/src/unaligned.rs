// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::Deref;

// Unaligned integer types are integers which Rust does not provide natively.
// This macro attempts to create wrapper types around the rounded up type
// supported, e.g. 24 -> 32.
//
// For the scope of the QUIC implementation 24-bit integers are needed
// for u24 encoded packet numbers:
// https://www.rfc-editor.org/rfc/rfc9000.html#name-packet-number-encoding-and-
//
// 48-bit integers are also implemented for completeness.
macro_rules! unaligned_integer_type {
    ($name:ident, $bitsize:expr, $storage_type:ty, $min:expr, $max:expr, [$($additional_conversions:ty),*]) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Default)]
        pub struct $name($storage_type);

        impl $name {
            pub const ZERO: Self = Self(0);
            pub const MIN: Self = Self($min);
            pub const MAX: Self = Self($max);

            /// Truncate the storage value into the allowed range
            #[inline]
            pub fn new_truncated(value: $storage_type) -> Self {
                Self(value & ((1 << $bitsize) - 1))
            }

            #[inline]
            pub fn from_be_bytes(bytes: [u8; ($bitsize / 8)]) -> Self {
                Self(UnalignedBytes::be_bytes_to_storage(bytes) as _)
            }

            #[inline]
            pub fn to_be_bytes(self) -> [u8; ($bitsize / 8)] {
                UnalignedBytes::storage_to_be_bytes(self.0 as _)
            }
        }

        #[cfg(any(test, feature = "generator"))]
        impl bolero_generator::TypeGenerator for $name {
            fn generate<D: bolero_generator::Driver>(driver: &mut D) -> Option<Self> {
                Some(Self::new_truncated(driver.gen()?))
            }
        }

        impl TryFrom<$storage_type> for $name {
            type Error = TryFromIntError;

            #[inline]
            fn try_from(value: $storage_type) -> Result<Self, Self::Error> {
                if value < (1 << $bitsize) {
                    Ok(Self(value))
                } else {
                    Err(TryFromIntError(()))
                }
            }
        }

        impl From<$name> for $storage_type {
            #[inline]
            fn from(value: $name) -> $storage_type {
                value.0
            }
        }

        $(
            impl From<$additional_conversions> for $name {
                #[inline]
                fn from(value: $additional_conversions) -> Self {
                    $name(value.into())
                }
            }

            impl From<$name> for $additional_conversions {
                #[inline]
                fn from(value: $name) -> Self {
                    value.0 as $additional_conversions
                }
            }
        )*

        impl Deref for $name {
            type Target = $storage_type;

            #[inline]
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

/// A trait defining how to convert between storage types and unaligned bytes
trait UnalignedBytes: Sized {
    type Storage;

    fn storage_to_be_bytes(storage: Self::Storage) -> Self;
    fn be_bytes_to_storage(self) -> Self::Storage;
}

impl UnalignedBytes for [u8; 3] {
    type Storage = u32;

    #[inline]
    fn storage_to_be_bytes(storage: Self::Storage) -> Self {
        let [_, a, b, c] = storage.to_be_bytes();
        [a, b, c]
    }

    #[inline]
    fn be_bytes_to_storage(self) -> Self::Storage {
        let [a, b, c] = self;
        let bytes = [0, a, b, c];
        Self::Storage::from_be_bytes(bytes)
    }
}

impl UnalignedBytes for [u8; 6] {
    type Storage = u64;

    #[inline]
    fn storage_to_be_bytes(storage: Self::Storage) -> Self {
        let [_, _, a, b, c, d, e, f] = storage.to_be_bytes();
        [a, b, c, d, e, f]
    }

    #[inline]
    fn be_bytes_to_storage(self) -> Self::Storage {
        let [a, b, c, d, e, f] = self;
        let bytes = [0, 0, a, b, c, d, e, f];
        Self::Storage::from_be_bytes(bytes)
    }
}

macro_rules! signed_min {
    ($bitsize:expr) => {
        -(1 << ($bitsize - 1))
    };
}

macro_rules! signed_max {
    ($bitsize:expr) => {
        ((1 << ($bitsize - 1)) - 1)
    };
}

#[test]
fn signed_min_max_test() {
    assert_eq!(i8::MIN as i16, signed_min!(8));
    assert_eq!(i8::MAX as i16, signed_max!(8));
}

unaligned_integer_type!(u24, 24, u32, 0, (1 << 24) - 1, [u8, u16]);
unaligned_integer_type!(
    i24,
    24,
    i32,
    signed_min!(24),
    signed_max!(24),
    [u8, i8, u16, i16]
);

impl TryFrom<u64> for u24 {
    type Error = TryFromIntError;

    #[inline]
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let storage_value: u32 = value.try_into()?;
        storage_value.try_into()
    }
}

impl From<u24> for u64 {
    #[inline]
    fn from(value: u24) -> u64 {
        value.0.into()
    }
}

unaligned_integer_type!(u48, 48, u64, 0, (1 << 48) - 1, [u8, u16, u32]);
unaligned_integer_type!(
    i48,
    48,
    i64,
    signed_min!(48),
    signed_max!(48),
    [u8, i8, u16, i16, u32, i32]
);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct TryFromIntError(());

impl From<core::num::TryFromIntError> for TryFromIntError {
    #[inline]
    fn from(_: core::num::TryFromIntError) -> Self {
        Self(())
    }
}

impl core::fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "TryFromIntError")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u8_len_3_be_bytes_to_storage() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &[u8; 3]| Some(callee.be_bytes_to_storage()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u8_len_6_be_bytes_to_storage() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &[u8; 6]| Some(callee.be_bytes_to_storage()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u24_new_truncated() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u32| Some(u24::new_truncated(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u24_ref_to_be_bytes() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &u24| Some(callee.to_be_bytes()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u32_try_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u32| Some(u24::try_from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u8_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u8| Some(u24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u16_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u16| Some(u24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u24_deref() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u24| Some(*callee.deref()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i32_new_truncated() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i32| Some(i24::new_truncated(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i24_to_be_bytes() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i24| Some(callee.to_be_bytes()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i32_try_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i32| Some(i24::try_from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i24_from_u8() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u8| Some(i24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i8_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i8| Some(i24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i24_from_u16() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u16| Some(i24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i16_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i16| Some(i24::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i24_deref() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i24| Some(*callee.deref()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u64_try_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u64| Some(u24::try_from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u64_new_truncated() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u64| Some(u48::new_truncated(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u48_to_be_bytes() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u48| Some(callee.to_be_bytes()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u48_try_from_u64() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u64| Some(u48::try_from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u8_from_u8() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u8| Some(u48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u48_from_u16() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u16| Some(u48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u32_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u32| Some(u48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u48_deref() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u48| Some(*callee.deref()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i64_new_truncated() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i64| Some(i48::new_truncated(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_to_be_bytes() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i48| Some(callee.to_be_bytes()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i64_try_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i64| Some(i48::try_from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_from_u8() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u8| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_from_i8() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i8| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_from_i48() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u16| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_from_i16() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i16| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_from_u32() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: u32| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i32_from() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|value: i32| Some(i48::from(value)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_deref() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i48| Some(*callee.deref()));
    }
}
