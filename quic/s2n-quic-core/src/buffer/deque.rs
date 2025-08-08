// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::slice::deque;
use alloc::{boxed::Box, vec::Vec};
use core::{fmt, mem::MaybeUninit};

mod storage;

#[cfg(test)]
mod tests;

/// A fixed-capacity ring buffer for bytes
#[derive(Clone)]
pub struct Deque {
    bytes: Box<[MaybeUninit<u8>]>,
    head: usize,
    len: usize,
}

impl fmt::Debug for Deque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deque")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}

impl From<Vec<u8>> for Deque {
    #[inline]
    fn from(mut buffer: Vec<u8>) -> Deque {
        let len = buffer.len();
        let mut capacity = buffer.capacity();
        if !capacity.is_power_of_two() {
            capacity = capacity.next_power_of_two();
            buffer.reserve_exact(capacity - len);
            debug_assert!(capacity.is_power_of_two());
        }

        unsafe {
            buffer.set_len(capacity);
        }

        let bytes = buffer.into_boxed_slice();
        let ptr = Box::into_raw(bytes);
        let bytes = unsafe { Box::from_raw(ptr as *mut [MaybeUninit<u8>]) };

        Self {
            bytes,
            head: 0,
            len,
        }
    }
}

impl Deque {
    #[inline]
    pub fn new(mut capacity: usize) -> Self {
        // Make sure capacity is set to a power of two
        // https://doc.rust-lang.org/std/primitive.usize.html#method.next_power_of_two
        //> Returns the smallest power of two greater than or equal to self.
        capacity = capacity.next_power_of_two();

        let mut bytes = Vec::<MaybeUninit<u8>>::with_capacity(capacity);
        unsafe {
            bytes.set_len(capacity);
        }
        let bytes = bytes.into_boxed_slice();

        Self {
            bytes,
            head: 0,
            len: 0,
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.bytes.len()
    }

    #[inline]
    pub fn remaining_capacity(&self) -> usize {
        self.preconditions();
        self.capacity() - self.len()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Resets the filled bytes in the buffer
    ///
    /// Note that data is not actually wiped with this method. If that behavior is desired then
    /// calling [`Self::consume_filled`] should be preferred.
    #[inline]
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    /// Consumes `len` bytes from the head of the buffer
    ///
    /// # Panics
    ///
    /// `len` MUST be less than or equal to [`Self::len`]
    #[inline]
    pub fn consume(&mut self, len: usize) {
        self.preconditions();

        assert!(self.len() >= len);

        if len >= self.len() {
            self.clear();
            return;
        }

        // Wrap the head around the capacity
        self.head = deque::wrap(&self.bytes, self.head, len);
        self.len -= len;

        self.postconditions()
    }

    /// Returns the filled bytes in the buffer
    #[inline]
    pub fn filled(&mut self) -> deque::Pair<&mut [u8]> {
        self.preconditions();

        unsafe {
            // SAFETY: cursors guarantee memory is filled
            deque::filled(&mut self.bytes, self.head, self.len).assume_init_mut()
        }
    }

    /// Returns and consumes `len` filled bytes in the buffer
    ///
    /// # Panics
    ///
    /// `len` MUST be less than or equal to [`Self::len`]
    #[inline]
    pub fn consume_filled(&mut self, len: usize) -> deque::Pair<&mut [u8]> {
        self.preconditions();

        let head = self.head;

        self.consume(len);

        self.postconditions();

        unsafe {
            // SAFETY: cursors guarantee memory is filled
            deque::filled(&mut self.bytes, head, len).assume_init_mut()
        }
    }

    /// Returns the unfilled bytes in the buffer
    ///
    /// Callers will need to call [`Self::fill`] to indicate any writes that occurred to returned
    /// slices.
    #[inline]
    pub fn unfilled(&mut self) -> deque::Pair<&mut [MaybeUninit<u8>]> {
        self.preconditions();
        deque::unfilled(&mut self.bytes, self.head, self.len)
    }

    /// Makes the buffer contiguous and contained in a single slice
    #[inline]
    pub fn make_contiguous(&mut self) -> &mut [u8] {
        self.preconditions();
        deque::make_contiguous(&mut self.bytes, &mut self.head, self.len);
        self.postconditions();

        let (head, tail) = self.filled().into();
        debug_assert!(tail.is_empty());
        head
    }

    /// Notifies the buffer that `len` bytes were written to it
    ///
    /// # Safety
    ///
    /// Callers must ensure the filled bytes were actually initialized
    #[inline]
    pub unsafe fn fill(&mut self, len: usize) -> Result<(), FillError> {
        ensure!(self.remaining_capacity() >= len, Err(FillError(())));

        self.len += len;

        self.postconditions();

        Ok(())
    }

    #[inline(always)]
    fn preconditions(&self) {
        unsafe {
            assume!(deque::invariants(&self.bytes, self.head, self.len));
            assume!(self.capacity().is_power_of_two());
        }
    }

    #[inline(always)]
    fn postconditions(&self) {
        debug_assert!(deque::invariants(&self.bytes, self.head, self.len));
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FillError(());

impl fmt::Display for FillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the buffer does not have enough capacity for the provided fill amount"
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for FillError {}

#[cfg(feature = "std")]
impl From<FillError> for std::io::Error {
    #[inline]
    fn from(value: FillError) -> Self {
        Self::new(std::io::ErrorKind::InvalidInput, value)
    }
}
