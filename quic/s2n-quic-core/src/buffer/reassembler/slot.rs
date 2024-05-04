// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{writer::Storage as _, Reader},
    varint::VarInt,
};
use bytes::{Buf, BufMut, BytesMut};
use core::fmt;

/// Possible states for slots in the [`Reassembler`]'s queue
#[derive(PartialEq, Eq)]
pub struct Slot {
    start: u64,
    end: u64,
    data: BytesMut,
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slot")
            .field("start", &self.start)
            .field("end", &self.end())
            .field("end_allocated", &self.end_allocated())
            .field("len", &self.data.len())
            .field("capacity", &self.data.capacity())
            .finish()
    }
}

impl Slot {
    #[inline]
    pub fn new(start: u64, end: u64, data: BytesMut) -> Self {
        super::probe::alloc(start, data.capacity());
        let v = Self { start, end, data };
        v.invariants();
        v
    }

    #[inline(always)]
    pub fn try_write_reader<R>(
        &mut self,
        reader: &mut R,
        filled_slot: &mut bool,
    ) -> Result<Option<Slot>, R::Error>
    where
        R: Reader + ?Sized,
    {
        debug_assert!(self.start() <= reader.current_offset().as_u64());

        let end = self.end();

        if end < self.end_allocated() {
            // trim off chunks we've already copied
            reader.skip_until(unsafe { VarInt::new_unchecked(end) })?;
        } else {
            // we've already filled this slot so skip the entire thing on the reader
            reader.skip_until(unsafe { VarInt::new_unchecked(self.end_allocated()) })?;
            return Ok(None);
        }

        ensure!(!reader.buffer_is_empty(), Ok(None));

        // make sure this slot owns this range of data
        ensure!(
            reader.current_offset().as_u64() < self.end_allocated(),
            Ok(None)
        );

        // if the current offsets match just do a straight copy
        if reader.current_offset().as_u64() == end {
            self.write_reader_end(reader, filled_slot)?;
            self.invariants();
            return Ok(None);
        }

        // split off the unfilled chunk from the filled chunk and return this filled one

        // find the split point between the buffers
        let unfilled_len = reader.current_offset().as_u64() - self.start();

        // create a new mid slot
        let start = reader.current_offset().as_u64();
        let data = unsafe {
            assume!(self.data.len() < unfilled_len as usize,);
            self.data.split_off(unfilled_len as usize)
        };

        let mut filled = Self {
            start,
            end: self.end,
            data,
        };

        // copy the data to the buffer
        if let Err(err) = filled.write_reader_end(reader, filled_slot) {
            // revert the split since the reader failed
            self.data.unsplit(filled.data);
            return Err(err);
        }

        self.end = start;

        self.invariants();
        filled.invariants();

        Ok(Some(filled))
    }

    #[inline(always)]
    fn write_reader_end<R>(
        &mut self,
        reader: &mut R,
        filled_slot: &mut bool,
    ) -> Result<(), R::Error>
    where
        R: Reader + ?Sized,
    {
        debug_assert_eq!(reader.current_offset().as_u64(), self.end());

        unsafe {
            // SAFETY: the data buffer should have at least one byte of spare capacity if we got to
            // this point
            assume!(self.data.capacity() > self.data.len());
        }
        let chunk = self.data.spare_capacity_mut();
        let mut chunk = bytes::buf::UninitSlice::uninit(chunk);
        let chunk_len = chunk.len();
        let mut chunk = chunk.track_write();
        reader.copy_into(&mut chunk)?;
        let len = chunk.written_len();

        super::probe::write(self.end(), len);

        unsafe {
            // SAFETY: we should not have written more than the spare capacity
            assume!(self.data.len() + len <= self.data.capacity());
            self.data.advance_mut(len);
        }
        *filled_slot |= chunk_len == len;

        Ok(())
    }

    #[inline]
    pub fn unsplit(&mut self, next: Self) {
        unsafe {
            assume!(self.end() == self.end_allocated());
            assume!(self.end() == next.start());
            assume!(!self.data.is_empty());
            assume!(self.data.capacity() > 0);
            assume!(!next.data.is_empty());
            assume!(next.data.capacity() > 0);
            assume!(self.data.as_ptr().add(self.data.len()) == next.data.as_ptr());
        }
        self.data.unsplit(next.data);
        self.end = next.end;

        self.invariants();
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.end() == self.end_allocated()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline(always)]
    pub fn is_occupied(&self, prev_offset: u64) -> bool {
        !self.is_empty() && self.start() == prev_offset
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut BytesMut {
        &mut self.data
    }

    #[inline(always)]
    pub fn start(&self) -> u64 {
        self.start
    }

    #[inline(always)]
    pub fn add_start(&mut self, len: usize) {
        self.start += len as u64;
        self.invariants()
    }

    #[inline(always)]
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }

    #[inline(always)]
    pub fn end_allocated(&self) -> u64 {
        self.end
    }

    #[inline]
    pub fn skip(&mut self, len: u64) {
        // trim off the data buffer
        unsafe {
            debug_assert!(len <= 1 << 16, "slot length should never exceed 2^16");
            let len = len as usize;

            // extend the write cursor if the length extends beyond the initialized offset
            if let Some(to_advance) = len.checked_sub(self.data.len()) {
                assume!(to_advance <= self.data.remaining_mut());
                self.data.advance_mut(to_advance);
            }

            // consume `len` bytes
            let to_advance = self.data.remaining().min(len);
            self.data.advance(to_advance);
        }

        // advance the start position
        self.start += len;

        self.invariants();
    }

    /// Indicates the slot isn't capable of storing any more data and should be dropped
    #[inline(always)]
    pub fn should_drop(&self) -> bool {
        self.start() == self.end_allocated()
    }

    #[inline(always)]
    fn invariants(&self) {
        if cfg!(debug_assertions) {
            assert!(self.data.capacity() <= 1 << 16, "{:?}", self);
            assert!(self.start() <= self.end(), "{:?}", self);
            assert!(self.start() <= self.end_allocated(), "{:?}", self);
            assert!(self.end() <= self.end_allocated(), "{:?}", self);
        }
    }
}
