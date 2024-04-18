// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    decoder::{
        buffer::{DecoderBuffer, DecoderBufferResult},
        buffer_mut::{DecoderBufferMut, DecoderBufferMutResult},
    },
    unaligned::{i24, i48, u24, u48},
};
use byteorder::{ByteOrder, NetworkEndian};
use core::mem::size_of;

pub trait DecoderValue<'a>: Sized {
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self>;
}

pub trait DecoderValueMut<'a>: Sized {
    fn decode_mut(bytes: DecoderBufferMut<'a>) -> DecoderBufferMutResult<Self>;
}

#[macro_export]
macro_rules! decoder_value {
    (impl<$lt:lifetime $(, $generic:ident)*> $ty:ty {
        fn decode($buffer:ident: Buffer) -> Result<$ret:ty> $impl:block
    }) => {
        impl<$lt $(, $generic: $crate::DecoderValue<$lt>)*> $crate::DecoderValue<$lt> for $ty {
            #[inline]
            fn decode($buffer: $crate::DecoderBuffer<$lt>) -> $crate::DecoderBufferResult<$lt, $ret> $impl
        }

        impl<$lt $(, $generic: $crate::DecoderValueMut<$lt>)*> $crate::DecoderValueMut<$lt> for $ty {
            #[inline]
            fn decode_mut($buffer: $crate::DecoderBufferMut<$lt>) -> $crate::DecoderBufferMutResult<$lt, $ret> $impl
        }
    };
}

macro_rules! decoder_value_byte {
    ($ty:ident) => {
        decoder_value!(
            impl<'a> $ty {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (value, buffer) = buffer.decode_slice(size_of::<Self>())?;
                    let value = value.as_less_safe_slice()[0] as $ty;
                    Ok((value, buffer))
                }
            }
        );
    };
}

decoder_value_byte!(u8);
decoder_value_byte!(i8);

macro_rules! decoder_value_network_endian {
    ($call:ident, $ty:ty) => {
        decoder_value!(
            impl<'a> $ty {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (value, buffer) = buffer.decode_slice(size_of::<Self>())?;
                    let value = value.as_less_safe_slice();
                    let value = NetworkEndian::$call(value);
                    Ok((value.into(), buffer))
                }
            }
        );
    };
}

decoder_value_network_endian!(read_u16, u16);
decoder_value_network_endian!(read_i16, i16);
decoder_value_network_endian!(read_u32, u32);
decoder_value_network_endian!(read_i32, i32);
decoder_value_network_endian!(read_u64, u64);
decoder_value_network_endian!(read_i64, i64);
decoder_value_network_endian!(read_u128, u128);
decoder_value_network_endian!(read_i128, i128);
decoder_value_network_endian!(read_f32, f32);
decoder_value_network_endian!(read_f64, f64);

macro_rules! decoder_value_unaligned_integer {
    ($call:ident, $ty:ident, $bitsize:expr) => {
        decoder_value!(
            impl<'a> $ty {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (value, buffer) = buffer.decode_slice($bitsize / 8)?;
                    let value = value.as_less_safe_slice();
                    let value = NetworkEndian::$call(value);
                    Ok(($ty::new_truncated(value), buffer))
                }
            }
        );
    };
}

decoder_value_unaligned_integer!(read_u24, u24, 24);
decoder_value_unaligned_integer!(read_i24, i24, 24);
decoder_value_unaligned_integer!(read_u48, u48, 48);
decoder_value_unaligned_integer!(read_i48, i48, 48);

decoder_value!(
    impl<'a> DecoderBuffer<'a> {
        fn decode(buffer: Buffer) -> Result<Self> {
            let len = buffer.len();
            let (slice, buffer) = buffer.decode_slice(len)?;
            #[allow(clippy::useless_conversion)]
            let slice = slice.into();
            Ok((slice, buffer))
        }
    }
);

decoder_value!(
    impl<'a> () {
        fn decode(buffer: Buffer) -> Result<Self> {
            Ok(((), buffer))
        }
    }
);

decoder_value!(
    impl<'a, T> Option<T> {
        fn decode(buffer: Buffer) -> Result<Self> {
            if buffer.is_empty() {
                Ok((None, buffer))
            } else {
                let (value, buffer) = buffer.decode()?;
                Ok((Some(value), buffer))
            }
        }
    }
);

impl<'a> DecoderValueMut<'a> for DecoderBufferMut<'a> {
    #[inline]
    fn decode_mut(buffer: DecoderBufferMut<'a>) -> DecoderBufferMutResult<'a, Self> {
        let len = buffer.len();
        buffer.decode_slice(len)
    }
}

/// A value whose decoding implementation can be altered
/// by a provided parameter.
pub trait DecoderParameterizedValue<'a>: Sized {
    type Parameter;

    fn decode_parameterized(
        parameter: Self::Parameter,
        bytes: DecoderBuffer<'a>,
    ) -> DecoderBufferResult<'a, Self>;
}

/// A mutable value whose decoding implementation can be altered
/// by a provided parameter.
pub trait DecoderParameterizedValueMut<'a>: Sized {
    type Parameter;

    fn decode_parameterized_mut(
        parameter: Self::Parameter,
        bytes: DecoderBufferMut<'a>,
    ) -> DecoderBufferMutResult<'a, Self>;
}

#[macro_export]
macro_rules! decoder_parameterized_value {
    (impl<$lt:lifetime $(, $generic:ident)*> $ty:ty {
        fn decode($tag:ident: $tag_ty:ty, $buffer:ident: Buffer) -> Result<$ret:ty> $impl:block
    }) => {
        impl<$lt $(, $generic: $crate::DecoderValue<$lt>)*> $crate::DecoderParameterizedValue<$lt> for $ty {
            type Parameter = $tag_ty;

            #[inline]
            fn decode_parameterized($tag: Self::Parameter, $buffer: $crate::DecoderBuffer<$lt>) -> $crate::DecoderBufferResult<$lt, $ret> $impl
        }

        impl<$lt $(, $generic: $crate::DecoderValueMut<$lt>)*> $crate::DecoderParameterizedValueMut<$lt> for $ty {
            type Parameter = $tag_ty;

            #[inline]
            fn decode_parameterized_mut($tag: Self::Parameter, $buffer: $crate::DecoderBufferMut<$lt>) -> $crate::DecoderBufferMutResult<$lt, $ret> $impl
        }
    };
}
