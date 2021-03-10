// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::decoder::{
    buffer::DecoderBuffer,
    value::{DecoderParameterizedValueMut, DecoderValueMut},
    DecoderError,
};

pub type DecoderBufferMutResult<'a, T> = Result<(T, DecoderBufferMut<'a>), DecoderError>;

/// DecoderBufferMut is a panic-free, mutable byte buffer for decoding untrusted input
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct DecoderBufferMut<'a> {
    bytes: &'a mut [u8],
}

impl<'a> DecoderBufferMut<'a> {
    /// Create a new `DecoderBufferMut` from a byte slice
    #[inline]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes }
    }

    /// Freeze the mutable buffer into a `DecoderBuffer`
    #[inline]
    pub fn freeze(self) -> DecoderBuffer<'a> {
        DecoderBuffer::new(self.bytes)
    }

    /// Move out the buffer's slice. This should be used with caution, as it
    /// removes any panic protection this struct provides.
    #[inline]
    pub fn into_less_safe_slice(self) -> &'a mut [u8] {
        self.bytes
    }

    /// Mutably borrow the buffer's slice. This should be used with caution, as it
    /// removes any panic protection this struct provides.
    #[inline]
    pub fn as_less_safe_slice_mut(&'a mut self) -> &'a mut [u8] {
        self.bytes
    }
}

impl_buffer!(
    DecoderBufferMut,
    DecoderBufferMutResult,
    DecoderValueMut,
    decode_mut,
    DecoderParameterizedValueMut,
    decode_parameterized_mut,
    split_at_mut
);

impl<'a> Into<DecoderBuffer<'a>> for DecoderBufferMut<'a> {
    fn into(self) -> DecoderBuffer<'a> {
        self.freeze()
    }
}
