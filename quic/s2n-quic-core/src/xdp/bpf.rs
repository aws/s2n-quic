// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::{DecoderBufferMut as Inner, DecoderError, DecoderValue, DecoderValueMut};

/// A BPF-aware version of [`s2n_codec::DecoderBufferMut`]
///
/// The Linux BPF verifier requires that every pointer be checked against the `end` pointer. This
/// means that it struggles with regular Rust slices that use `ptr + len` instead of `ptr + end`.
pub struct DecoderBufferMut<'a> {
    buffer: Inner<'a>,
    end: *mut u8,
}

impl<'a> DecoderBufferMut<'a> {
    /// Creates a new DecoderBufferMut.
    ///
    /// # Safety
    ///
    /// The `start` and `end` pointers must be a valid range of bytes, ideally directly coming
    /// from the BPF/XDP context argument.
    #[inline]
    pub unsafe fn new(start: *mut u8, end: *mut u8) -> Self {
        let len = end as usize - start as usize;
        let data = core::slice::from_raw_parts_mut(start as *mut u8, len);
        let buffer = Inner::new(data);
        Self { buffer, end }
    }

    /// Validates that the starting pointer is still within the bounds of the end pointer
    #[inline]
    fn new_checked(buffer: Inner<'a>, end: *mut u8) -> Result<Self, DecoderError> {
        // The Linux BPF verifier needs to prove that no pointers go beyond the "end" pointer
        if buffer.as_less_safe_slice().as_ptr() > end {
            return Err(DecoderError::UnexpectedEof(0));
        }

        Ok(Self { buffer, end })
    }

    /// Decodes a T from the buffer, if possible
    #[inline]
    pub fn decode<T: DecoderValueMut<'a>>(self) -> Result<(T, Self), DecoderError> {
        let end = self.end;
        let (v, buffer) = self.buffer.decode()?;
        let buffer = Self::new_checked(buffer, end)?;
        Ok((v, buffer))
    }

    /// Decodes a slice of bytes with the given len, if possible
    #[inline]
    pub fn decode_slice(self, len: usize) -> Result<(Self, Self), DecoderError> {
        let end = self.end;
        let (slice, buffer) = self.buffer.decode_slice(len)?;
        let slice = Self::new_checked(slice, end)?;
        let buffer = Self::new_checked(buffer, end)?;
        Ok((slice, buffer))
    }
}

/// A generic interface over a decoder buffer
pub trait Decoder<'a>: Sized {
    fn decode<T: DecoderValue<'a> + DecoderValueMut<'a>>(
        self,
    ) -> core::result::Result<(T, Self), DecoderError>;
    fn decode_slice(self, len: usize) -> core::result::Result<(Self, Self), DecoderError>;
}

impl<'a> Decoder<'a> for DecoderBufferMut<'a> {
    #[inline]
    fn decode<T: DecoderValue<'a> + DecoderValueMut<'a>>(
        self,
    ) -> core::result::Result<(T, Self), DecoderError> {
        Self::decode(self)
    }

    #[inline]
    fn decode_slice(self, len: usize) -> core::result::Result<(Self, Self), DecoderError> {
        Self::decode_slice(self, len)
    }
}

impl<'a> Decoder<'a> for s2n_codec::DecoderBuffer<'a> {
    #[inline]
    fn decode<T: DecoderValue<'a> + DecoderValueMut<'a>>(
        self,
    ) -> core::result::Result<(T, Self), DecoderError> {
        Self::decode(self)
    }

    #[inline]
    fn decode_slice(self, len: usize) -> core::result::Result<(Self, Self), DecoderError> {
        Self::decode_slice(self, len)
    }
}

impl<'a> Decoder<'a> for s2n_codec::DecoderBufferMut<'a> {
    #[inline]
    fn decode<T: DecoderValue<'a> + DecoderValueMut<'a>>(
        self,
    ) -> core::result::Result<(T, Self), DecoderError> {
        Self::decode(self)
    }

    #[inline]
    fn decode_slice(self, len: usize) -> core::result::Result<(Self, Self), DecoderError> {
        Self::decode_slice(self, len)
    }
}
