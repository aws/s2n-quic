// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::encoder::Encoder;

/// EncoderBuffer is a buffer for writing to a mutable slice
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct EncoderBuffer<'a> {
    bytes: &'a mut [u8],
    position: usize,
}

impl<'a> EncoderBuffer<'a> {
    /// Creates a new `EncoderBuffer`
    #[inline]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Sets the write cursor to a new position
    ///
    /// # Panics
    /// Panics when `position > capacity`
    #[inline]
    pub fn set_position(&mut self, position: usize) {
        debug_assert!(position <= self.capacity());
        self.position = position;
    }

    /// Advances the write cursor by offset
    ///
    /// # Panics
    /// Panics when `position > capacity`
    #[inline]
    pub fn advance_position(&mut self, offset: usize) {
        let position = self.position + offset;
        self.set_position(position)
    }

    /// Splits off the used buffer from the remaining bytes
    #[inline]
    pub fn split_off(self) -> (&'a mut [u8], &'a mut [u8]) {
        self.bytes.split_at_mut(self.position)
    }

    /// Splits the used buffer from the remaining bytes
    #[inline]
    pub fn split_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        self.bytes.split_at_mut(self.position)
    }

    /// Returns the written bytes as a mutable slice
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes[..self.position]
    }

    #[inline]
    fn assert_capacity(&self, len: usize) {
        debug_assert!(
            len <= self.remaining_capacity(),
            "not enough buffer capacity. wanted: {}, available: {}",
            len,
            self.remaining_capacity()
        );
    }
}

impl<'a> Encoder for EncoderBuffer<'a> {
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, write: F) {
        self.assert_capacity(len);
        let end = self.position + len;
        write(&mut self.bytes[self.position..end]);
        self.position = end;
    }

    fn write_slice(&mut self, slice: &[u8]) {
        self.assert_capacity(slice.len());
        let position = self.position;
        let len = slice.len();
        let end = position + len;
        self.bytes[position..end].copy_from_slice(slice);
        self.position = end;
    }

    fn write_repeated(&mut self, count: usize, value: u8) {
        self.assert_capacity(count);
        let start = self.position;
        let end = start + count;
        for byte in &mut self.bytes[start..end] {
            *byte = value;
        }
        self.position = end;
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }

    fn len(&self) -> usize {
        self.position
    }
}
