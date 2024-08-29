// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    encoder::{Encoder, EncoderLenEstimator},
    i24, i48, u24, u48, DecoderBuffer, DecoderBufferMut,
};
use byteorder::{ByteOrder, NetworkEndian};
use core::mem::size_of;

pub trait EncoderValue: Sized {
    /// Encodes the value into the encoder
    fn encode<E: Encoder>(&self, encoder: &mut E);

    /// Encodes the value into the encoder, while potentially mutating the value itself
    #[inline]
    fn encode_mut<E: Encoder>(&mut self, encoder: &mut E) {
        self.encode(encoder)
    }

    /// Returns the encoding size with no buffer constrains
    #[inline]
    fn encoding_size(&self) -> usize {
        self.encoding_size_for_encoder(&EncoderLenEstimator::new(usize::MAX))
    }

    /// Returns the encoding size for the given encoder's capacity
    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, encoder: &E) -> usize {
        let mut estimator = EncoderLenEstimator::new(encoder.remaining_capacity());
        self.encode(&mut estimator);
        estimator.len()
    }

    /// Encodes the value into the encoder with a prefix of `Len`
    #[inline]
    fn encode_with_len_prefix<Len: TryFrom<usize> + EncoderValue, E: Encoder>(
        &self,
        encoder: &mut E,
    ) where
        Self: Sized,
        Len::Error: core::fmt::Debug,
    {
        let len = self.encoding_size_for_encoder(encoder);
        let len: Len = Len::try_from(len).expect("invalid conversion");
        len.encode(encoder);
        self.encode(encoder);
    }

    #[cfg(feature = "alloc")]
    fn encode_to_vec(&self) -> alloc::vec::Vec<u8> {
        let len = self.encoding_size();
        let mut buffer = alloc::vec![0u8; len];
        self.encode(&mut crate::EncoderBuffer::new(&mut buffer));
        buffer
    }
}

macro_rules! encoder_value_byte {
    ($ty:ident) => {
        impl EncoderValue for $ty {
            #[inline]
            fn encode<E: Encoder>(&self, encoder: &mut E) {
                encoder.write_sized(size_of::<Self>(), |buf| {
                    buf[0] = *self as u8;
                })
            }

            #[inline]
            fn encoding_size(&self) -> usize {
                size_of::<Self>()
            }

            #[inline]
            fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
                size_of::<Self>()
            }
        }
    };
}

encoder_value_byte!(u8);
encoder_value_byte!(i8);

macro_rules! encoder_value_network_endian {
    ($call:ident, $ty:ty, $size:expr) => {
        impl EncoderValue for $ty {
            #[inline]
            fn encode<E: Encoder>(&self, encoder: &mut E) {
                encoder.write_sized($size, |buf| {
                    NetworkEndian::$call(buf, (*self).into());
                })
            }

            #[inline]
            fn encoding_size(&self) -> usize {
                $size
            }

            #[inline]
            fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
                $size
            }
        }
    };
}

encoder_value_network_endian!(write_u16, u16, size_of::<Self>());
encoder_value_network_endian!(write_i16, i16, size_of::<Self>());
encoder_value_network_endian!(write_u24, u24, 3);
encoder_value_network_endian!(write_i24, i24, 3);
encoder_value_network_endian!(write_u32, u32, size_of::<Self>());
encoder_value_network_endian!(write_i32, i32, size_of::<Self>());
encoder_value_network_endian!(write_u48, u48, 6);
encoder_value_network_endian!(write_i48, i48, 6);
encoder_value_network_endian!(write_u64, u64, size_of::<Self>());
encoder_value_network_endian!(write_i64, i64, size_of::<Self>());
encoder_value_network_endian!(write_u128, u128, size_of::<Self>());
encoder_value_network_endian!(write_i128, i128, size_of::<Self>());
encoder_value_network_endian!(write_f32, f32, size_of::<Self>());
encoder_value_network_endian!(write_f64, f64, size_of::<Self>());

macro_rules! encoder_value_slice {
    ($ty:ty, |$self:ident| $value:expr) => {
        impl EncoderValue for $ty {
            #[inline]
            fn encode<E: Encoder>(&$self, encoder: &mut E) {
                encoder.write_slice($value)
            }

            #[inline]
            fn encoding_size(&self) -> usize {
                self.len()
            }

            #[inline]
            fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
                self.len()
            }
        }
    };
}

encoder_value_slice!(&[u8], |self| self);
encoder_value_slice!(&mut [u8], |self| self);
encoder_value_slice!(DecoderBuffer<'_>, |self| self.as_less_safe_slice());
encoder_value_slice!(DecoderBufferMut<'_>, |self| self.as_less_safe_slice());

impl EncoderValue for &'_ [&'_ [u8]] {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        for slice in self.iter() {
            encoder.write_slice(slice)
        }
    }

    #[inline]
    fn encoding_size(&self) -> usize {
        self.iter().map(|s| s.len()).sum()
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        self.iter().map(|s| s.len()).sum()
    }
}

impl EncoderValue for () {
    #[inline]
    fn encode<E: Encoder>(&self, _encoder: &mut E) {}

    #[inline]
    fn encoding_size(&self) -> usize {
        0
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        0
    }
}

impl<A: EncoderValue, B: EncoderValue> EncoderValue for (A, B) {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.0.encode(encoder);
        self.1.encode(encoder);
    }

    #[inline]
    fn encoding_size(&self) -> usize {
        self.0.encoding_size() + self.1.encoding_size()
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, encoder: &E) -> usize {
        self.0.encoding_size_for_encoder(encoder) + self.1.encoding_size_for_encoder(encoder)
    }
}

impl<T: EncoderValue> EncoderValue for Option<T> {
    #[inline]
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        if let Some(value) = self.as_ref() {
            value.encode(buffer);
        }
    }

    #[inline]
    fn encode_mut<E: Encoder>(&mut self, buffer: &mut E) {
        if let Some(value) = self.as_mut() {
            value.encode_mut(buffer);
        }
    }
}

#[cfg(feature = "bytes")]
impl EncoderValue for bytes::Bytes {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        if E::SPECIALIZES_BYTES {
            encoder.write_bytes(self.clone())
        } else {
            encoder.write_slice(self)
        }
    }

    #[inline]
    fn encoding_size(&self) -> usize {
        self.len()
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        self.len()
    }
}

#[cfg(feature = "bytes")]
impl EncoderValue for &bytes::Bytes {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        if E::SPECIALIZES_BYTES {
            encoder.write_bytes((*self).clone())
        } else {
            encoder.write_slice(self)
        }
    }

    #[inline]
    fn encoding_size(&self) -> usize {
        self.len()
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        self.len()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u8_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u8| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i8_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i8| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u16_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u16| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i16_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i16| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u24_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u24| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i24_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i24| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u32_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u32| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i32_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i32| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u48_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u48| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i48_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i48| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u64_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u64| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i64_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i64| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn u128_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: u128| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn i128_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: i128| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn f32_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: f32| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn f64_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: f64| Some(callee.encoding_size()));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn empty_encoding_size() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: ()| Some(callee.encoding_size()));
    }
}
