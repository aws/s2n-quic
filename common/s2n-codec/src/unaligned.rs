use core::{
    convert::{TryFrom, TryInto},
    ops::Deref,
};

// Unaligned integer types are integers which Rust does not provide natively.
// This macro attempts to create wrapper types around the rounded up type
// supported, e.g. 24 -> 32.
//
// For the scope of the QUIC implementation 24-bit integers are needed
// for u24 encoded packet numbers:
// https://tools.ietf.org/html/draft-ietf-quic-transport-22#section-17.1
//
// 48-bit integers are also implemented for completeness.
macro_rules! unaligned_integer_type {
    ($name:ident, $bitsize:expr, $storage_type:ty, [$($additional_conversions:ty),*]) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Default)]
        pub struct $name(pub(crate) $storage_type);

        impl $name {
            /// Truncate the storage value into the allowed range
            pub fn new_truncated(value: $storage_type) -> Self {
                Self(value & ((1 << $bitsize) - 1))
            }
        }

        #[cfg(feature = "generator")]
        impl bolero_generator::TypeGenerator for $name {
            fn generate<D: bolero_generator::Driver>(driver: &mut D) -> Option<Self> {
                Some(Self::new_truncated(driver.gen()?))
            }
        }

        impl TryFrom<$storage_type> for $name {
            type Error = TryFromIntError;

            fn try_from(value: $storage_type) -> Result<Self, Self::Error> {
                if value < (1 << $bitsize) {
                    Ok(Self(value))
                } else {
                    Err(TryFromIntError(()))
                }
            }
        }

        impl Into<$storage_type> for $name {
            fn into(self) -> $storage_type {
                self.0
            }
        }

        $(
            impl From<$additional_conversions> for $name {
                fn from(value: $additional_conversions) -> Self {
                    $name(value.into())
                }
            }

            impl Into<$additional_conversions> for $name {
                fn into(self) -> $additional_conversions {
                    self.0 as $additional_conversions
                }
            }
        )*

        impl Deref for $name {
            type Target = $storage_type;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

unaligned_integer_type!(u24, 24, u32, [u8, u16]);
unaligned_integer_type!(i24, 24, i32, [u8, i8, u16, i16]);

impl TryFrom<u64> for u24 {
    type Error = TryFromIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        let storage_value: u32 = value.try_into()?;
        storage_value.try_into()
    }
}

impl Into<u64> for u24 {
    fn into(self) -> u64 {
        self.0.into()
    }
}

unaligned_integer_type!(u48, 24, u64, [u8, u16, u32]);
unaligned_integer_type!(i48, 24, i64, [u8, i8, u16, i16, u32, i32]);

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TryFromIntError(());

impl From<core::num::TryFromIntError> for TryFromIntError {
    fn from(_: core::num::TryFromIntError) -> Self {
        Self(())
    }
}

impl core::fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "TryFromIntError")
    }
}
