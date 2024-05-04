// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{reader, writer::Storage as _, Reader},
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

        // read the current offset
        let start = reader.current_offset().as_u64();

        // make sure this slot owns this range of data
        ensure!(start < self.end_allocated(), Ok(None));

        // if the current offsets match just do a straight copy on to the end of the buffer
        if start == end {
            self.write_reader_append(reader, filled_slot)?;
            self.invariants();
            return Ok(None);
        }

        // copy and split off the filled data into another slot
        let filled = self.write_reader_split(reader, filled_slot)?;

        self.invariants();
        filled.invariants();

        Ok(Some(filled))
    }

    #[inline(always)]
    fn write_reader_split<R>(
        &mut self,
        reader: &mut R,
        filled_slot: &mut bool,
    ) -> Result<Self, R::Error>
    where
        R: Reader + ?Sized,
    {
        let reader_start = reader.current_offset().as_u64();

        unsafe {
            assume!(reader_start > self.end());
        }
        let offset = reader_start - self.end();

        let chunk = self.data.spare_capacity_mut();

        unsafe {
            // SAFETY: the data buffer should have at least one byte of spare capacity if we got to
            // this point
            assume!(chunk.len() as u64 > offset);
        }

        let chunk = &mut chunk[offset as usize..];
        let mut chunk = bytes::buf::UninitSlice::uninit(chunk);
        let chunk_len = chunk.len();
        let mut chunk = chunk.track_write();
        reader.copy_into(&mut chunk)?;
        let filled_len = chunk.written_len();

        super::probe::write(reader_start, filled_len);

        let filled = unsafe {
            // SAFETY: we should not have written more than the spare capacity
            let offset = offset as usize;

            assume!(self.data.len() + offset <= self.data.capacity());
            let mut filled = self.data.split_off(self.data.len() + offset);

            assume!(filled.is_empty());
            assume!(filled_len <= filled.capacity() - filled.len());
            filled.advance_mut(filled_len);
            filled
        };
        *filled_slot |= chunk_len == filled_len;

        let filled = Self {
            start: reader_start,
            end: self.end,
            data: filled,
        };
        filled.invariants();

        self.end = reader_start;
        self.invariants();

        Ok(filled)
    }

    #[inline(always)]
    fn write_reader_append<R>(
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
    pub fn start(&self) -> u64 {
        self.start
    }

    #[inline(always)]
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }

    #[inline(always)]
    pub fn end_allocated(&self) -> u64 {
        self.end
    }

    #[inline(always)]
    pub fn consume(&mut self) -> BytesMut {
        let data = core::mem::replace(&mut self.data, BytesMut::new());
        self.start = self.end;
        data
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
            assert!(self.data.capacity() <= 1 << 16, "{self:?}");
            assert!(self.start() <= self.end(), "{self:?}");
            assert!(self.start() <= self.end_allocated(), "{self:?}");
            assert!(self.end() <= self.end_allocated(), "{self:?}");
            assert_eq!(
                self.data.capacity() as u64,
                self.end_allocated() - self.start(),
                "{self:?}"
            );
            assert_eq!(
                self.data.len() as u64,
                self.end() - self.start(),
                "{self:?}"
            );
        }
    }
}

impl reader::Storage for Slot {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.data.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.data.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk<'_>, Self::Error> {
        let chunk = self.data.read_chunk(watermark)?;
        self.start += chunk.len() as u64;
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized,
    {
        let mut dest = dest.track_write();
        let chunk = self.data.partial_copy_into(&mut dest)?;
        self.start += dest.written_len() as u64;
        self.start += chunk.len() as u64;
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized,
    {
        let mut dest = dest.track_write();
        self.data.copy_into(&mut dest)?;
        self.start += dest.written_len() as u64;
        self.invariants();
        Ok(())
    }
}

impl Reader for Slot {
    #[inline]
    fn current_offset(&self) -> VarInt {
        unsafe { VarInt::new_unchecked(self.start) }
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        Some(unsafe { VarInt::new_unchecked(self.end) })
    }

    #[inline]
    fn skip_until(&mut self, offset: VarInt) -> Result<(), Self::Error> {
        if let Some(len) = offset.as_u64().checked_sub(self.current_offset().as_u64()) {
            self.skip(len);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{buffer::reader::testing::Fallible, stream::testing::Data};
    use bolero::{check, TypeGenerator};

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    struct Params {
        slot_offset: VarInt,
        slot_filled: u16,
        slot_len: u16,
        skip_len: u16,
        reader_offset: VarInt,
        reader_len: u16,
        is_error: bool,
    }

    impl Params {
        fn run(&self) {
            let mut slot = self.slot();
            let mut reader = self.reader(&slot);
            let mut reader = reader.with_read_limit(self.reader_len as _);

            if self.is_error {
                let mut reader = Fallible::new(&mut reader).with_error(());
                let _ = slot.try_write_reader(&mut reader, &mut false);
            } else {
                let _ = slot.try_write_reader(&mut reader, &mut false);
            }
        }

        fn slot(&self) -> Slot {
            let start = self.slot_offset.as_u64();
            let end = start + self.slot_len as u64;
            let end = VarInt::MAX.as_u64().min(end);
            let len = end - start;
            let mut bytes = BytesMut::with_capacity(len as _);

            // fill some bytes
            let filled_len = len.min(self.slot_filled as _) as usize;
            bytes.resize(filled_len, 0);

            let mut slot = Slot::new(start, end, bytes);

            // skip some bytes
            let skip_len = len.min(self.skip_len as u64);
            slot.skip(skip_len);

            slot
        }

        fn reader(&self, slot: &Slot) -> Data {
            let mut reader = Data::new(u64::MAX);
            // the reader needs to be at least start at the same offset as the slot
            let start = self.reader_offset.as_u64().max(slot.start);
            reader.seek_forward(start);
            reader
        }
    }

    #[test]
    fn try_write_test() {
        check!().with_type::<Params>().for_each(|params| {
            params.run();
        });
    }
}
