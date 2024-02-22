// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains data structures for buffering incoming streams.

use crate::{
    buffer::{Error, Reader},
    varint::VarInt,
};
use alloc::collections::{vec_deque, VecDeque};
use bytes::BytesMut;

mod duplex;
mod probe;
mod reader;
mod request;
mod slot;
mod writer;

#[cfg(test)]
mod tests;

use request::Request;
use slot::Slot;

/// The default buffer size for slots that the [`Reassembler`] uses.
///
/// This value was picked as it is typically used for the default memory page size.
const MIN_BUFFER_ALLOCATION_SIZE: usize = 4096;

/// The value used for when the final size is unknown.
///
/// By using `u64::MAX` we don't have to special case any of the logic. Also note that the actual
/// max size of any stream is a `VarInt::MAX` so this isn't a valid value.
const UNKNOWN_FINAL_SIZE: u64 = u64::MAX;

//= https://www.rfc-editor.org/rfc/rfc9000#section-2.2
//# Endpoints MUST be able to deliver stream data to an application as an
//# ordered byte-stream.

/// `Reassembler` is a buffer structure for combining chunks of bytes in an
/// ordered stream, which might arrive out of order.
///
/// `Reassembler` will accumulate the bytes, and provide them to its users
/// once a contiguous range of bytes at the current position of the stream has
/// been accumulated.
///
/// `Reassembler` is optimized for minimizing memory allocations and for
/// offering it's users chunks of sizes that minimize call overhead.
///
/// If data is received in smaller chunks, only the first chunk will trigger a
/// memory allocation. All other chunks can be copied into the already allocated
/// region.
///
/// When users want to consume data from the buffer, the consumable part of the
/// internal receive buffer is split off and passed back to the caller. Due to
/// this chunk being a view onto a reference-counted internal buffer of type
/// [`BytesMut`] this is also efficient and does not require additional memory
/// allocation or copy.
///
/// ## Usage
///
/// ```rust
/// use s2n_quic_core::buffer::Reassembler;
///
/// let mut buffer = Reassembler::new();
///
/// // write a chunk of bytes at offset 4, which can not be consumed yet
/// assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
/// assert_eq!(0, buffer.len());
/// assert_eq!(None, buffer.pop());
///
/// // write a chunk of bytes at offset 0, which allows for consumption
/// assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
/// assert_eq!(8, buffer.len());
///
/// // Pop chunks. Since they all fitted into a single internal buffer,
/// // they will be returned in combined fashion.
/// assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
/// ```
#[derive(Debug, PartialEq, Default)]
pub struct Reassembler {
    slots: VecDeque<Slot>,
    cursors: Cursors,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Cursors {
    start_offset: u64,
    max_recv_offset: u64,
    final_offset: u64,
}

impl Default for Cursors {
    #[inline]
    fn default() -> Self {
        Self {
            start_offset: 0,
            max_recv_offset: 0,
            final_offset: UNKNOWN_FINAL_SIZE,
        }
    }
}

impl Reassembler {
    /// Creates a new `Reassembler`
    #[inline]
    pub fn new() -> Reassembler {
        Self::default()
    }

    /// Returns true if the buffer has completely been written to and the final size is known
    #[inline]
    pub fn is_writing_complete(&self) -> bool {
        self.final_size()
            .map_or(false, |len| self.total_received_len() == len)
    }

    /// Returns true if the buffer has completely been read and the final size is known
    #[inline]
    pub fn is_reading_complete(&self) -> bool {
        self.final_size()
            .map_or(false, |len| self.cursors.start_offset == len)
    }

    /// Returns the final size of the stream, if known
    #[inline]
    pub fn final_size(&self) -> Option<u64> {
        if self.cursors.final_offset == UNKNOWN_FINAL_SIZE {
            None
        } else {
            Some(self.cursors.final_offset)
        }
    }

    /// Returns the amount of bytes available for reading.
    /// This equals the amount of data that is stored in contiguous fashion at
    /// the start of the buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.report().0
    }

    /// Returns true if no bytes are available for reading
    #[inline]
    pub fn is_empty(&self) -> bool {
        if let Some(slot) = self.slots.front() {
            !slot.is_occupied(self.cursors.start_offset)
        } else {
            true
        }
    }

    /// Returns the number of bytes and chunks available for consumption
    #[inline]
    pub fn report(&self) -> (usize, usize) {
        let mut bytes = 0;
        let mut chunks = 0;
        for chunk in self.iter() {
            bytes += chunk.len();
            chunks += 1;
        }
        (bytes, chunks)
    }

    /// Pushes a slice at a certain offset
    #[inline]
    pub fn write_at(&mut self, offset: VarInt, data: &[u8]) -> Result<(), Error> {
        let mut request = Request::new(offset, data, false)?;
        self.write_reader(&mut request)?;
        Ok(())
    }

    /// Pushes a slice at a certain offset, which is the end of the buffer
    #[inline]
    pub fn write_at_fin(&mut self, offset: VarInt, data: &[u8]) -> Result<(), Error> {
        let mut request = Request::new(offset, data, true)?;
        self.write_reader(&mut request)?;
        Ok(())
    }

    #[inline]
    pub fn write_reader<R>(&mut self, reader: &mut R) -> Result<(), Error<R::Error>>
    where
        R: Reader + ?Sized,
    {
        // Trims off any data that has already been received
        reader.skip_until(self.current_offset())?;

        // store a snapshot of the cursors in case there's an error
        let snapshot = self.cursors;

        self.check_reader_fin(reader)?;

        if let Err(err) = self.write_reader_impl(reader) {
            use core::any::TypeId;
            if TypeId::of::<R::Error>() != TypeId::of::<core::convert::Infallible>() {
                self.cursors = snapshot;
            }
            return Err(Error::ReaderError(err));
        }

        self.invariants();

        Ok(())
    }

    /// Ensures the final offset doesn't change
    #[inline]
    fn check_reader_fin<R>(&mut self, reader: &mut R) -> Result<(), Error<R::Error>>
    where
        R: Reader + ?Sized,
    {
        let buffered_offset = reader
            .current_offset()
            .checked_add_usize(reader.buffered_len())
            .ok_or(Error::OutOfRange)?
            .as_u64();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
        //# Once a final size for a stream is known, it cannot change.  If a
        //# RESET_STREAM or STREAM frame is received indicating a change in the
        //# final size for the stream, an endpoint SHOULD respond with an error
        //# of type FINAL_SIZE_ERROR; see Section 11 for details on error
        //# handling.
        match (reader.final_offset(), self.final_size()) {
            (Some(actual), Some(expected)) => {
                ensure!(actual == expected, Err(Error::InvalidFin));
            }
            (Some(final_offset), None) => {
                let final_offset = final_offset.as_u64();

                // make sure that we didn't see any previous chunks greater than the final size
                ensure!(
                    self.cursors.max_recv_offset <= final_offset,
                    Err(Error::InvalidFin)
                );

                self.cursors.final_offset = final_offset;
            }
            (None, Some(expected)) => {
                // make sure the reader doesn't exceed a previously known final offset
                ensure!(expected >= buffered_offset, Err(Error::InvalidFin));
            }
            (None, None) => {}
        }

        // record the maximum offset that we've seen
        self.cursors.max_recv_offset = self.cursors.max_recv_offset.max(buffered_offset);

        Ok(())
    }

    #[inline(always)]
    fn write_reader_impl<R>(&mut self, reader: &mut R) -> Result<(), R::Error>
    where
        R: Reader + ?Sized,
    {
        // if the reader is empty at this point, just make sure it doesn't return an error
        if reader.buffer_is_empty() {
            let _chunk = reader.read_chunk(0)?;
            return Ok(());
        }

        let mut selected = None;

        // start from the back with the assumption that most data arrives in order
        for idx in (0..self.slots.len()).rev() {
            let Some(slot) = self.slots.get(idx) else {
                debug_assert!(false);
                unsafe {
                    // SAFETY: `idx` should always be in bounds, since it's generated by the range
                    // `0..slots.len()`
                    core::hint::unreachable_unchecked()
                }
            };

            // find the first slot that we can write into
            ensure!(slot.start() <= reader.current_offset().as_u64(), continue);

            selected = Some(idx);
            break;
        }

        let idx = if let Some(idx) = selected {
            idx
        } else {
            let mut idx = 0;
            // set the current request to the upper slot and loop
            let mut slot = self.allocate_slot(reader);

            // before pushing the slot, make sure the reader doesn't fail
            let filled = slot.try_write_reader(reader, &mut true)?;

            if let Some(slot) = filled {
                self.slots.push_front(slot);
                idx += 1;
            }
            self.slots.push_front(slot);

            ensure!(!reader.buffer_is_empty(), Ok(()));

            idx
        };

        self.write_reader_at(reader, idx)?;
        Ok(())
    }

    #[inline(always)]
    fn write_reader_at<R>(&mut self, reader: &mut R, mut idx: usize) -> Result<(), R::Error>
    where
        R: Reader + ?Sized,
    {
        let initial_idx = idx;
        let mut filled_slot = false;

        unsafe {
            assume!(
                !reader.buffer_is_empty(),
                "the first write should always be non-empty"
            );
        }

        while !reader.buffer_is_empty() {
            let Some(slot) = self.slots.get_mut(idx) else {
                debug_assert!(false);
                unsafe { core::hint::unreachable_unchecked() }
            };

            let filled = slot.try_write_reader(reader, &mut filled_slot)?;

            idx += 1;
            if let Some(slot) = filled {
                self.insert(idx, slot);
                idx += 1;
            }

            ensure!(!reader.buffer_is_empty(), break);

            if let Some(next) = self.slots.get(idx) {
                // the next slot is able to handle the reader
                if next.start() <= reader.current_offset().as_u64() {
                    continue;
                }
            }

            let slot = self.allocate_slot(reader);
            self.insert(idx, slot);
            continue;
        }

        // only try unsplitting if we filled at least one spot
        if filled_slot {
            self.unsplit_range(initial_idx..idx);
        }

        Ok(())
    }

    #[inline]
    fn unsplit_range(&mut self, range: core::ops::Range<usize>) {
        // try to merge all of the slots that were modified
        for idx in range.rev() {
            let Some(slot) = self.slots.get(idx) else {
                debug_assert!(false);
                unsafe {
                    // SAFETY: `idx` should always be in bounds, since it's provided by a range
                    // that was bound to `slots.len()`
                    core::hint::unreachable_unchecked()
                }
            };

            // if this slot was completed, we should try and unsplit with the next slot
            ensure!(slot.is_full(), continue);

            let start = slot.start();
            let end = slot.end();

            let Some(next) = self.slots.get(idx + 1) else {
                continue;
            };

            ensure!(next.start() == end, continue);

            let current_block = Self::align_offset(start, Self::allocation_size(start));
            let next_block = Self::align_offset(next.start(), Self::allocation_size(next.start()));
            ensure!(current_block == next_block, continue);

            if let Some(next) = self.slots.remove(idx + 1) {
                self.slots[idx].unsplit(next);
            } else {
                debug_assert!(false, "idx + 1 was checked above");
                unsafe { core::hint::unreachable_unchecked() }
            }
        }
    }

    /// Advances the read and write cursors and discards any held data
    ///
    /// This can be used for copy-avoidance applications where a packet is received in order and
    /// doesn't need to be stored temporarily for future packets to unblock the stream.
    #[inline]
    pub fn skip(&mut self, len: VarInt) -> Result<(), Error> {
        // zero-length skip is a no-op
        ensure!(len > VarInt::ZERO, Ok(()));

        let new_start_offset = self
            .cursors
            .start_offset
            .checked_add(len.as_u64())
            .ok_or(Error::OutOfRange)?;

        if let Some(final_size) = self.final_size() {
            ensure!(final_size >= new_start_offset, Err(Error::InvalidFin));
        }

        // record the maximum offset that we've seen
        self.cursors.max_recv_offset = self.cursors.max_recv_offset.max(new_start_offset);

        // update the current start offset
        self.cursors.start_offset = new_start_offset;

        // clear out the slots to the new start offset
        while let Some(mut slot) = self.slots.pop_front() {
            // the new offset consumes the slot so drop and continue
            if slot.end_allocated() < new_start_offset {
                continue;
            }

            match new_start_offset.checked_sub(slot.start()) {
                None | Some(0) => {
                    // the slot starts after/on the new offset so put it back and break out
                    self.slots.push_front(slot);
                }
                Some(len) => {
                    // the slot overlaps with the new boundary so modify it and put it back if
                    // needed
                    slot.skip(len);

                    if !slot.should_drop() {
                        self.slots.push_front(slot);
                    }
                }
            }

            break;
        }

        self.invariants();

        Ok(())
    }

    /// Iterates over all of the chunks waiting to be received
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        Iter::new(self)
    }

    /// Drains all of the currently available chunks
    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = BytesMut> + '_ {
        Drain { inner: self }
    }

    /// Pops a buffer from the front of the receive queue if available
    #[inline]
    pub fn pop(&mut self) -> Option<BytesMut> {
        self.pop_transform(|buffer, is_final_offset| {
            let chunk = if is_final_offset || buffer.len() == buffer.capacity() {
                core::mem::take(buffer)
            } else {
                buffer.split()
            };
            let len = chunk.len();
            (chunk, len)
        })
    }

    /// Pops a buffer from the front of the receive queue, who's length is always guaranteed to be
    /// less than the provided `watermark`.
    #[inline]
    pub fn pop_watermarked(&mut self, watermark: usize) -> Option<BytesMut> {
        self.pop_transform(|buffer, is_final_offset| {
            // make sure the buffer doesn't exceed the watermark
            let watermark = watermark.min(buffer.len());

            // if the watermark is 0 then don't needlessly increment refcounts
            ensure!(watermark > 0, (BytesMut::new(), 0));

            if watermark == buffer.len() && is_final_offset {
                return (core::mem::take(buffer), watermark);
            }

            (buffer.split_to(watermark), watermark)
        })
    }

    /// Pops a buffer from the front of the receive queue as long as the `transform` function returns a
    /// non-empty buffer.
    #[inline]
    fn pop_transform<F: FnOnce(&mut BytesMut, bool) -> (O, usize), O>(
        &mut self,
        transform: F,
    ) -> Option<O> {
        let slot = self.slots.front_mut()?;

        // make sure the slot has some data
        ensure!(slot.is_occupied(self.cursors.start_offset), None);

        let is_final_offset = self.cursors.final_offset == slot.end();
        let buffer = slot.data_mut();

        let (out, len) = transform(buffer, is_final_offset);

        // filter out empty buffers
        ensure!(len > 0, None);

        slot.add_start(len);

        if slot.should_drop() {
            // remove empty buffers
            self.slots.pop_front();
        }

        probe::pop(self.cursors.start_offset, len);

        self.cursors.start_offset += len as u64;

        self.invariants();

        Some(out)
    }

    /// Returns the amount of data that had already been consumed from the
    /// receive buffer.
    #[inline]
    pub fn consumed_len(&self) -> u64 {
        self.cursors.start_offset
    }

    /// Returns the total amount of contiguous received data.
    ///
    /// This includes the already consumed data as well as the data that is still
    /// buffered and available for consumption.
    #[inline]
    pub fn total_received_len(&self) -> u64 {
        let mut offset = self.cursors.start_offset;

        for slot in &self.slots {
            ensure!(slot.is_occupied(offset), offset);
            offset = slot.end();
        }

        offset
    }

    /// Resets the receive buffer.
    ///
    /// This will drop all previously received data.
    #[inline]
    pub fn reset(&mut self) {
        self.slots.clear();
        self.cursors = Default::default();
    }

    #[inline(always)]
    fn insert(&mut self, idx: usize, slot: Slot) {
        if self.slots.len() < idx {
            debug_assert_eq!(self.slots.len() + 1, idx);
            self.slots.push_back(slot);
        } else {
            self.slots.insert(idx, slot);
        }
    }

    /// Allocates a slot for a reader
    #[inline]
    fn allocate_slot<R>(&mut self, reader: &R) -> Slot
    where
        R: Reader + ?Sized,
    {
        let start = reader.current_offset().as_u64();
        let mut size = Self::allocation_size(start);
        let mut offset = Self::align_offset(start, size);

        // don't allocate for data we've already consumed
        if let Some(diff) = self.cursors.start_offset.checked_sub(offset) {
            if diff > 0 {
                debug_assert!(
                    reader.current_offset().as_u64() >= self.cursors.start_offset,
                    "requests should be split before allocating slots"
                );
                offset = self.cursors.start_offset;
                size -= diff as usize;
            }
        }

        if self.cursors.final_offset
            - reader.current_offset().as_u64()
            - reader.buffered_len() as u64
            == 0
        {
            let size_candidate = (start - offset) as usize + reader.buffered_len();
            if size_candidate < size {
                size = size_candidate;
            }
        }

        let buffer = BytesMut::with_capacity(size);

        let end = offset + size as u64;
        Slot::new(offset, end, buffer)
    }

    /// Aligns an offset to a certain alignment size
    #[inline(always)]
    fn align_offset(offset: u64, alignment: usize) -> u64 {
        unsafe {
            assume!(alignment > 0);
        }
        (offset / (alignment as u64)) * (alignment as u64)
    }

    /// Returns the desired allocation size for the given offset
    ///
    /// The allocation size gradually increases as the offset increases. This is under
    /// the assumption that streams that receive a lot of data will continue to receive
    /// a lot of data.
    ///
    /// The current table is as follows:
    ///
    /// | offset         | allocation size |
    /// |----------------|-----------------|
    /// | 0              | 4096            |
    /// | 65536          | 16384           |
    /// | 262144         | 32768           |
    /// | >=1048575      | 65536           |
    #[inline(always)]
    fn allocation_size(offset: u64) -> usize {
        for pow in (2..=4).rev() {
            let mult = 1 << pow;
            let square = mult * mult;
            let min_offset = (MIN_BUFFER_ALLOCATION_SIZE * square) as u64;
            let allocation_size = MIN_BUFFER_ALLOCATION_SIZE * mult;

            if offset >= min_offset {
                return allocation_size;
            }
        }

        MIN_BUFFER_ALLOCATION_SIZE
    }

    #[inline(always)]
    fn invariants(&self) {
        if cfg!(debug_assertions) {
            assert_eq!(
                self.total_received_len(),
                self.consumed_len() + self.len() as u64
            );

            let (actual_len, chunks) = self.report();

            assert_eq!(actual_len == 0, self.is_empty());
            assert_eq!(self.iter().count(), chunks);

            let mut prev_end = self.cursors.start_offset;

            for (idx, slot) in self.slots.iter().enumerate() {
                assert!(slot.start() >= prev_end, "{self:#?}");
                assert!(!slot.should_drop(), "slot range should be non-empty");
                prev_end = slot.end_allocated();

                // make sure if the slot is full, then it was unsplit into the next slot
                if slot.is_full() {
                    let start = slot.start();
                    let end = slot.end();

                    let Some(next) = self.slots.get(idx + 1) else {
                        continue;
                    };

                    ensure!(next.start() == end, continue);

                    let current_block = Self::align_offset(start, Self::allocation_size(start));
                    let next_block =
                        Self::align_offset(next.start(), Self::allocation_size(next.start()));
                    ensure!(current_block == next_block, continue);

                    panic!("unmerged slots at {idx} and {} {self:#?}", idx + 1);
                }
            }
        }
    }
}

pub struct Iter<'a> {
    prev_end: u64,
    inner: vec_deque::Iter<'a, Slot>,
}

impl<'a> Iter<'a> {
    #[inline]
    fn new(buffer: &'a Reassembler) -> Self {
        Self {
            prev_end: buffer.cursors.start_offset,
            inner: buffer.slots.iter(),
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let slot = self.inner.next()?;

        ensure!(slot.is_occupied(self.prev_end), None);

        self.prev_end = slot.end();
        Some(slot.as_slice())
    }
}

pub struct Drain<'a> {
    inner: &'a mut Reassembler,
}

impl<'a> Iterator for Drain<'a> {
    type Item = BytesMut;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.pop()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.inner.slots.len();
        (len, Some(len))
    }
}
