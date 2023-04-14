// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use ::byteorder::NetworkEndian;
use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};
pub use zerocopy::*;

#[cfg(feature = "generator")]
use bolero_generator::*;

/// Define a codec implementation for a zerocopy value that implements
/// `FromBytes`, `AsBytes`, and `Unaligned`.
#[macro_export]
macro_rules! zerocopy_value_codec {
    ($name:ident) => {
        impl<'a> $crate::DecoderValue<'a> for $name
        where
            $name: $crate::zerocopy::FromBytes,
        {
            #[inline]
            fn decode(buffer: $crate::DecoderBuffer<'a>) -> $crate::DecoderBufferResult<Self> {
                let (value, buffer) = <&'a $name as $crate::DecoderValue>::decode(buffer)?;
                Ok((*value, buffer))
            }
        }

        impl<'a> $crate::DecoderValue<'a> for &'a $name
        where
            $name: $crate::zerocopy::FromBytes,
        {
            #[inline]
            fn decode(buffer: $crate::DecoderBuffer<'a>) -> $crate::DecoderBufferResult<Self> {
                let (value, buffer) = buffer.decode_slice(core::mem::size_of::<$name>())?;
                let value = value.into_less_safe_slice();
                let value = unsafe {
                    // Safety: the type implements FromBytes
                    &*(value as *const _ as *const $name)
                };
                Ok((value, buffer.into()))
            }
        }

        impl<'a> $crate::DecoderValueMut<'a> for $name
        where
            $name: $crate::zerocopy::FromBytes,
        {
            #[inline]
            fn decode_mut(
                buffer: $crate::DecoderBufferMut<'a>,
            ) -> $crate::DecoderBufferMutResult<Self> {
                let (value, buffer) = <&'a $name as $crate::DecoderValueMut>::decode_mut(buffer)?;
                Ok((*value, buffer))
            }
        }

        impl<'a> $crate::DecoderValueMut<'a> for &'a $name
        where
            $name: $crate::zerocopy::FromBytes,
        {
            #[inline]
            fn decode_mut(
                buffer: $crate::DecoderBufferMut<'a>,
            ) -> $crate::DecoderBufferMutResult<'a, Self> {
                let (value, buffer) =
                    <&'a mut $name as $crate::DecoderValueMut>::decode_mut(buffer)?;
                Ok((value, buffer))
            }
        }

        impl<'a> $crate::DecoderValueMut<'a> for &'a mut $name
        where
            $name: $crate::zerocopy::FromBytes,
        {
            #[inline]
            fn decode_mut(
                buffer: $crate::DecoderBufferMut<'a>,
            ) -> $crate::DecoderBufferMutResult<'a, Self> {
                let (value, buffer) = buffer.decode_slice(core::mem::size_of::<$name>())?;
                let value = value.into_less_safe_slice();
                let value = unsafe {
                    // Safety: the type implements FromBytes
                    &mut *(value as *mut _ as *mut $name)
                };

                Ok((value, buffer.into()))
            }
        }

        impl $crate::EncoderValue for $name
        where
            $name: $crate::zerocopy::AsBytes,
        {
            #[inline]
            fn encoding_size(&self) -> usize {
                core::mem::size_of::<$name>()
            }

            #[inline]
            fn encoding_size_for_encoder<E: $crate::Encoder>(&self, _encoder: &E) -> usize {
                core::mem::size_of::<$name>()
            }

            #[inline]
            fn encode<E: $crate::Encoder>(&self, encoder: &mut E) {
                let bytes = unsafe {
                    // Safety: the type implements AsBytes
                    core::slice::from_raw_parts(
                        self as *const $name as *const u8,
                        core::mem::size_of::<$name>(),
                    )
                };
                encoder.write_slice(bytes);
            }
        }

        impl<'a> $crate::EncoderValue for &'a $name
        where
            $name: $crate::zerocopy::AsBytes,
        {
            #[inline]
            fn encoding_size(&self) -> usize {
                core::mem::size_of::<$name>()
            }

            #[inline]
            fn encoding_size_for_encoder<E: $crate::Encoder>(&self, _encoder: &E) -> usize {
                ::core::mem::size_of::<$name>()
            }

            #[inline]
            fn encode<E: $crate::Encoder>(&self, encoder: &mut E) {
                let bytes = unsafe {
                    // Safety: the type implements AsBytes
                    core::slice::from_raw_parts(
                        *self as *const $name as *const u8,
                        core::mem::size_of::<$name>(),
                    )
                };
                encoder.write_slice(bytes);
            }
        }

        impl<'a> $crate::EncoderValue for &'a mut $name
        where
            $name: $crate::zerocopy::AsBytes,
        {
            #[inline]
            fn encoding_size(&self) -> usize {
                core::mem::size_of::<$name>()
            }

            #[inline]
            fn encoding_size_for_encoder<E: $crate::Encoder>(&self, _encoder: &E) -> usize {
                ::core::mem::size_of::<$name>()
            }

            #[inline]
            fn encode<E: $crate::Encoder>(&self, encoder: &mut E) {
                let bytes = unsafe {
                    // Safety: the type implements AsBytes
                    core::slice::from_raw_parts(
                        *self as *const $name as *const u8,
                        core::mem::size_of::<$name>(),
                    )
                };
                encoder.write_slice(bytes);
            }
        }
    };
}

// The `zerocopy` crate provides integer types that are able to be referenced
// in an endian-independent method. This macro wraps those types and implements
// a few convenience traits.
macro_rules! zerocopy_network_integer {
    ($native:ident, $name:ident) => {
        #[derive(
            Clone,
            Copy,
            Default,
            Eq,
            $crate::zerocopy::FromBytes,
            $crate::zerocopy::AsBytes,
            $crate::zerocopy::Unaligned,
        )]
        #[repr(C)]
        pub struct $name(::zerocopy::byteorder::$name<NetworkEndian>);

        impl $name {
            pub const ZERO: Self = Self(::zerocopy::byteorder::$name::ZERO);

            #[inline]
            pub fn new(value: $native) -> Self {
                value.into()
            }

            #[inline]
            pub fn get(&self) -> $native {
                self.0.get()
            }

            #[inline]
            pub fn set(&mut self, value: $native) {
                self.0.set(value);
            }
        }

        impl PartialEq for $name {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.cmp(other) == Ordering::Equal
            }
        }

        impl PartialEq<$native> for $name {
            #[inline]
            fn eq(&self, other: &$native) -> bool {
                self.partial_cmp(other) == Some(Ordering::Equal)
            }
        }

        impl PartialOrd for $name {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl PartialOrd<$native> for $name {
            #[inline]
            fn partial_cmp(&self, other: &$native) -> Option<Ordering> {
                Some(self.0.get().cmp(other))
            }
        }

        impl Ord for $name {
            #[inline]
            fn cmp(&self, other: &Self) -> Ordering {
                self.0.get().cmp(&other.0.get())
            }
        }

        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.get().hash(state);
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "{}", self.0.get())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "{}", self.0.get())
            }
        }

        impl From<$native> for $name {
            #[inline]
            fn from(value: $native) -> Self {
                Self(::zerocopy::byteorder::$name::new(value))
            }
        }

        impl From<$name> for $native {
            #[inline]
            fn from(v: $name) -> $native {
                v.0.get()
            }
        }

        #[cfg(feature = "generator")]
        impl TypeGenerator for $name {
            fn generate<D: Driver>(driver: &mut D) -> Option<Self> {
                Some(Self::new(driver.gen()?))
            }
        }

        zerocopy_value_codec!($name);
    };
}

zerocopy_network_integer!(i16, I16);
zerocopy_network_integer!(u16, U16);
zerocopy_network_integer!(i32, I32);
zerocopy_network_integer!(u32, U32);
zerocopy_network_integer!(i64, I64);
zerocopy_network_integer!(u64, U64);
zerocopy_network_integer!(i128, I128);
zerocopy_network_integer!(u128, U128);

#[test]
fn zerocopy_struct_test() {
    use crate::DecoderBuffer;

    #[derive(Copy, Clone, Debug, PartialEq, PartialOrd, FromBytes, AsBytes, Unaligned)]
    #[repr(C)]
    struct UdpHeader {
        source_port: U16,
        destination_port: U16,
        payload_len: U16,
        checksum: U16,
    }

    zerocopy_value_codec!(UdpHeader);

    let buffer = vec![0, 1, 0, 2, 0, 3, 0, 4];
    let decoder = DecoderBuffer::new(&buffer);
    let (mut header, _) = decoder.decode().unwrap();

    ensure_codec_round_trip_value!(UdpHeader, header).unwrap();
    ensure_codec_round_trip_value!(&UdpHeader, &header).unwrap();
    ensure_codec_round_trip_value_mut!(&mut UdpHeader, &mut header).unwrap();

    assert_eq!(header.source_port, 1u16);
    assert_eq!(header.destination_port, 2u16);
    assert_eq!(header.payload_len, 3u16);
    assert_eq!(header.checksum, 4u16);
}
