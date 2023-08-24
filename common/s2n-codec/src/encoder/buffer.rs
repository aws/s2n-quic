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
        debug_assert!(
            position <= self.capacity(),
            "position {position} exceeded capacity of {}",
            self.capacity()
        );
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
        unsafe { self.bytes.get_unchecked_mut(..self.position) }
    }

    #[inline]
    pub(crate) fn assert_capacity(&self, len: usize) {
        debug_assert!(
            len <= self.remaining_capacity(),
            "not enough buffer capacity. wanted: {}, available: {}",
            len,
            self.remaining_capacity()
        );
    }
}

impl<'a> Encoder for EncoderBuffer<'a> {
    #[inline]
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, write: F) {
        self.assert_capacity(len);
        let end = self.position + len;
        let bytes = unsafe {
            // Safety: bounds already checked
            self.bytes.get_unchecked_mut(self.position..end)
        };
        write(bytes);
        self.position = end;
    }

    #[inline]
    fn write_slice(&mut self, slice: &[u8]) {
        self.write_sized(slice.len(), |dest| dest.copy_from_slice(slice));
    }

    #[inline]
    fn write_repeated(&mut self, count: usize, value: u8) {
        self.write_sized(count, |dest| {
            for byte in dest {
                *byte = value;
            }
        })
    }

    #[inline]
    fn write_zerocopy<
        T: zerocopy::AsBytes + zerocopy::FromBytes + zerocopy::Unaligned,
        F: FnOnce(&mut T),
    >(
        &mut self,
        write: F,
    ) {
        let len = core::mem::size_of::<T>();
        self.write_sized(len, |bytes| {
            let value = unsafe {
                // The `zerocopy` markers ensure this is a safe operation
                &mut *(bytes as *mut _ as *mut T)
            };
            write(value)
        })
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.bytes.len()
    }

    #[inline]
    fn len(&self) -> usize {
        self.position
    }
}
