// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod buffer;
pub mod estimator;
pub mod scatter;
pub mod value;

pub use buffer::*;
pub use estimator::*;
pub use value::*;

use core::convert::TryFrom;

pub trait Encoder: Sized {
    /// Set to `true` if the particular encoder specializes on the bytes implementation
    #[cfg(feature = "bytes")]
    const SPECIALIZES_BYTES: bool = false;

    /// Encode the given `EncoderValue` into the buffer
    #[inline]
    fn encode<T: EncoderValue>(&mut self, value: &T) {
        value.encode(self)
    }

    /// Encode the given `EncoderValue` into the buffer with a prefix of `Len`
    #[inline]
    fn encode_with_len_prefix<Len: TryFrom<usize> + EncoderValue, T: EncoderValue>(
        &mut self,
        value: &T,
    ) where
        Len::Error: core::fmt::Debug,
    {
        value.encode_with_len_prefix::<Len, Self>(self)
    }

    /// Calls `write` with a slice of `len` bytes at the current write position
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, write: F);

    /// Copies the slice into the buffer
    fn write_slice(&mut self, slice: &[u8]);

    #[cfg(feature = "bytes")]
    #[inline]
    fn write_bytes(&mut self, bytes: bytes::Bytes) {
        self.write_slice(&bytes)
    }

    /// Writes a zerocopy value to the buffer
    fn write_zerocopy<
        T: zerocopy::AsBytes + zerocopy::FromBytes + zerocopy::Unaligned,
        F: FnOnce(&mut T),
    >(
        &mut self,
        write: F,
    );

    /// Repeatedly write a byte `value` for a given `count`
    ///
    /// ```
    /// # use s2n_codec::*;
    /// let mut buffer = vec![255; 1024];
    /// let mut encoder = EncoderBuffer::new(&mut buffer);
    /// encoder.encode(&1u8);
    /// encoder.write_repeated(4, 0);
    /// assert_eq!(&buffer[0..6], &[1, 0, 0, 0, 0, 255]);
    /// ```
    fn write_repeated(&mut self, count: usize, value: u8);

    /// Returns the total buffer capacity
    fn capacity(&self) -> usize;

    /// Returns the number of bytes written to the buffer
    fn len(&self) -> usize;

    /// Returns `true` if no bytes have been written
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of available bytes in the buffer
    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.capacity().saturating_sub(self.len())
    }
}
