// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    decoder::{
        buffer::{DecoderBuffer, DecoderBufferResult},
        buffer_mut::{DecoderBufferMut, DecoderBufferMutResult},
    },
    unaligned::{i24, i48, u24, u48},
    DecoderError,
};
use byteorder::{ByteOrder, NetworkEndian};
use core::{marker::PhantomData, mem::size_of};
use zerocopy::{FromBytes, Immutable, Unaligned};

pub trait DecoderValue<'a>: Sized {
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self>;
}

pub trait DecoderValueMut<'a>: Sized {
    fn decode_mut(bytes: DecoderBufferMut<'a>) -> DecoderBufferMutResult<'a, Self>;
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

/// PrefixedBlob is a length-prefixed string of bytes.
///
/// This is particularly useful for TLS messages. For example, the
/// `opaque legacy_compression_methods<1..2^8-1>` field from the TLS 1.3 RFC could
/// be decoded as `PrefixedBlob<'a, u8>`.
pub struct PrefixedBlob<'a, L> {
    pub blob: &'a [u8],
    phantom_length: PhantomData<L>,
}

impl<'a, L: Into<usize> + DecoderValue<'a>> DecoderValue<'a> for PrefixedBlob<'a, L> {
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self> {
        let (length, buffer): (L, DecoderBuffer) = bytes.decode()?;
        let length: usize = length.into();

        let (blob, buffer) = buffer.decode_slice(length)?;
        let blob = blob.into_less_safe_slice();

        let value = Self {
            blob,
            phantom_length: PhantomData,
        };

        Ok((value, buffer))
    }
}

/// A PrefixedList represents a length prefixed list, with a length prefix of `L`
/// and elements of type `T`.
///
/// Note that this will neither allocate nor copy `T`, so it must be valid to directly
/// construct them from the underlying `&[u8]`.
///
/// This type is particularly useful for representing TLS messages, such as a list
/// of supported `NamedGroup` items in the Supported Groups extension.
pub struct PrefixedList<'a, L, T> {
    pub list: &'a [T],
    phantom_length: PhantomData<L>,
}

impl<'a, L: Into<usize> + DecoderValue<'a>, T: FromBytes + Immutable + Unaligned> DecoderValue<'a>
    for PrefixedList<'a, L, T>
{
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self> {
        let (length, buffer): (L, DecoderBuffer) = bytes.decode()?;
        let length: usize = length.into();
        let (blob, buffer) = buffer.decode_slice(length)?;
        let blob = blob.into_less_safe_slice();
        let list = FromBytes::ref_from_bytes(blob).map_err(|_| {
            DecoderError::InvariantViolation("blob length is not a multiple of element size")
        })?;

        let value = Self {
            list,
            phantom_length: PhantomData,
        };

        Ok((value, buffer))
    }
}

// This implementation will not allocate data, but will copy it onto the stack
impl<'a, const N: usize> DecoderValue<'a> for [u8; N] {
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self> {
        let (value, buffer) = bytes.decode_slice(N)?;
        let value = value.into_less_safe_slice().try_into().map_err(|_| {
            DecoderError::InvariantViolation("decode_slice returned a slice of the wrong length")
        })?;
        Ok((value, buffer))
    }
}

impl<'a, const N: usize> DecoderValue<'a> for &'a [u8; N] {
    fn decode(bytes: DecoderBuffer<'a>) -> DecoderBufferResult<'a, Self> {
        let (value, buffer) = bytes.decode_slice(N)?;
        let slice = value.into_less_safe_slice();
        let value = slice.try_into().map_err(|_| {
            DecoderError::InvariantViolation("decode_slice returned a slice of the wrong length")
        })?;
        Ok((value, buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecoderBuffer;

    #[test]
    fn array_decode() {
        let buf = DecoderBuffer::new(&[1, 2, 3, 4, 5]);
        let (val, remaining) = buf.decode::<[u8; 3]>().unwrap();
        assert_eq!(val, [1, 2, 3]);
        assert_eq!(remaining.into_less_safe_slice(), &[4, 5]);

        let buf = DecoderBuffer::new(&[]);
        let (val, remaining) = buf.decode::<[u8; 0]>().unwrap();
        assert_eq!(val, []);
        assert!(remaining.is_empty());

        // error: insufficient data
        assert!(DecoderBuffer::new(&[1, 2]).decode::<[u8; 4]>().is_err());
    }

    #[test]
    fn ref_array_decode() {
        let buf = DecoderBuffer::new(&[1, 2, 3, 4, 5]);
        let (val, remaining) = buf.decode::<&[u8; 3]>().unwrap();
        assert_eq!(val, &[1, 2, 3]);
        assert_eq!(remaining.into_less_safe_slice(), &[4, 5]);

        let buf = DecoderBuffer::new(&[]);
        let (val, remaining) = buf.decode::<&[u8; 0]>().unwrap();
        assert_eq!(val, &[]);
        assert!(remaining.is_empty());

        // error: insufficient data
        assert!(DecoderBuffer::new(&[1, 2]).decode::<&[u8; 4]>().is_err());
    }

    #[test]
    fn prefixed_blob_decode() {
        // u8 length prefix
        let buf = DecoderBuffer::new(&[3, 0xAA, 0xBB, 0xCC]);
        let (blob, remaining) = buf.decode::<PrefixedBlob<u8>>().unwrap();
        assert_eq!(blob.blob, &[0xAA, 0xBB, 0xCC]);
        assert!(remaining.is_empty());

        // u16 big-endian length = 256 (0x01, 0x00) — verifies endianness
        let mut data = vec![0x01, 0x00];
        data.extend(core::iter::repeat(0xFFu8).take(256));
        let buf = DecoderBuffer::new(&data);
        let (blob, remaining) = buf.decode::<PrefixedBlob<u16>>().unwrap();
        assert_eq!(blob.blob.len(), 256);
        assert!(remaining.is_empty());

        // errors: length exceeds data, missing/truncated length prefix
        assert!(DecoderBuffer::new(&[5, 0x01, 0x02])
            .decode::<PrefixedBlob<u8>>()
            .is_err());
        assert!(DecoderBuffer::new(&[0x01, 0x00, 0xAA])
            .decode::<PrefixedBlob<u16>>()
            .is_err());
        assert!(DecoderBuffer::new(&[0x01])
            .decode::<PrefixedBlob<u16>>()
            .is_err());
    }

    #[test]
    fn prefixed_list_decode() {
        // u8 length prefix, u8 elements
        let buf = DecoderBuffer::new(&[3, 10, 20, 30]);
        let (list, remaining) = buf.decode::<PrefixedList<u8, u8>>().unwrap();
        assert_eq!(list.list, &[10, 20, 30]);
        assert!(remaining.is_empty());

        // u16 length prefix, u16 big-endian elements — verifies endianness of both
        let buf = DecoderBuffer::new(&[0x00, 0x04, 0x01, 0x02, 0x03, 0x04]);
        let (list, remaining) = buf
            .decode::<PrefixedList<u16, zerocopy::network_endian::U16>>()
            .unwrap();
        assert_eq!(list.list.len(), 2);
        assert_eq!(list.list[0].get(), 0x0102);
        assert_eq!(list.list[1].get(), 0x0304);
        assert!(remaining.is_empty());

        // errors: insufficient data
        assert!(DecoderBuffer::new(&[5, 1, 2])
            .decode::<PrefixedList<u8, u8>>()
            .is_err());
        assert!(DecoderBuffer::new(&[0x00, 0x03, 0x01, 0x02, 0x03])
            .decode::<PrefixedList<u16, zerocopy::network_endian::U16>>()
            .is_err());
    }
}
