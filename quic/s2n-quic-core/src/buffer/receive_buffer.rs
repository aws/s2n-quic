// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains data structures for buffering incoming and outgoing data
//! in Quic streams.

use crate::varint::VarInt;
use alloc::collections::{vec_deque, VecDeque};
use bytes::BytesMut;
use core::fmt;

mod request;
mod slot;

#[cfg(test)]
mod tests;

use request::Request;
use slot::Slot;

/// Enumerates error that can occur while inserting data into the Receive Buffer
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ReceiveBufferError {
    /// An invalid data range was provided
    OutOfRange,
    /// The provided final size was invalid for the buffer's state
    InvalidFin,
}

#[cfg(feature = "std")]
impl std::error::Error for ReceiveBufferError {}

impl fmt::Display for ReceiveBufferError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::OutOfRange => write!(f, "write extends out of the maximum possible offset"),
            Self::InvalidFin => write!(
                f,
                "write modifies the final offset in a non-compliant manner"
            ),
        }
    }
}

/// The default buffer size for slots that the [`ReceiveBuffer`] uses.
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

/// `ReceiveBuffer` is a buffer structure for combining chunks of bytes in an
/// ordered stream, which might arrive out of order.
///
/// `ReceiveBuffer` will accumulate the bytes, and provide them to its users
/// once a contiguous range of bytes at the current position of the stream has
/// been accumulated.
///
/// `ReceiveBuffer` is optimized for minimizing memory allocations and for
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
/// ```rust,ignore
/// use s2n_quic_transport::buffer::ReceiveBuffer;
///
/// let mut buffer = ReceiveBuffer::new();
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
#[derive(Debug, PartialEq)]
pub struct ReceiveBuffer {
    slots: VecDeque<Slot>,
    start_offset: u64,
    max_recv_offset: u64,
    final_offset: u64,
}

impl Default for ReceiveBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiveBuffer {
    /// Creates a new `ReceiveBuffer`
    pub fn new() -> ReceiveBuffer {
        ReceiveBuffer {
            slots: VecDeque::new(),
            start_offset: 0,
            max_recv_offset: 0,
            final_offset: UNKNOWN_FINAL_SIZE,
        }
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
            .map_or(false, |len| self.start_offset == len)
    }

    /// Returns the final size of the stream, if known
    #[inline]
    pub fn final_size(&self) -> Option<u64> {
        if self.final_offset == UNKNOWN_FINAL_SIZE {
            None
        } else {
            Some(self.final_offset)
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
        self.len() == 0
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
    pub fn write_at(&mut self, offset: VarInt, data: &[u8]) -> Result<(), ReceiveBufferError> {
        // set the `fin` flag if this write ends at the known final size
        let is_fin = if let Some(final_size) = self.final_size() {
            offset + data.len() == final_size
        } else {
            false
        };

        // create a request
        let request = Request::new(offset, data, is_fin)?;
        self.write_request(request)?;
        Ok(())
    }

    /// Pushes a slice at a certain offset, which is the end of the buffer
    #[inline]
    pub fn write_at_fin(&mut self, offset: VarInt, data: &[u8]) -> Result<(), ReceiveBufferError> {
        // create a request
        let request = Request::new(offset, data, true)?;

        // compute the final offset for the fin request
        let final_offset = request.end_exclusive();

        // make sure if we previously saw a final size that they still match
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
        //# Once a final size for a stream is known, it cannot change.  If a
        //# RESET_STREAM or STREAM frame is received indicating a change in the
        //# final size for the stream, an endpoint SHOULD respond with an error
        //# of type FINAL_SIZE_ERROR; see Section 11 for details on error
        //# handling.
        if let Some(final_size) = self.final_size() {
            ensure!(
                final_size == final_offset,
                Err(ReceiveBufferError::InvalidFin)
            );
        }

        // make sure that we didn't see any previous chunks greater than the final size
        ensure!(
            self.max_recv_offset <= final_offset,
            Err(ReceiveBufferError::InvalidFin)
        );

        self.final_offset = final_offset;

        self.write_request(request)?;

        Ok(())
    }

    #[inline]
    fn write_request(&mut self, request: Request) -> Result<(), ReceiveBufferError> {
        // trim off any data that we've already read
        let (_, request) = request.split(self.start_offset);
        // trim off any data that exceeds our final length
        let (mut request, excess) = request.split(self.final_offset);

        // make sure the request isn't trying to write beyond the final size
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
        //# Once a final size for a stream is known, it cannot change.  If a
        //# RESET_STREAM or STREAM frame is received indicating a change in the
        //# final size for the stream, an endpoint SHOULD respond with an error
        //# of type FINAL_SIZE_ERROR; see Section 11 for details on error
        //# handling.
        ensure!(excess.is_empty(), Err(ReceiveBufferError::InvalidFin));

        // if the request is empty we're done
        ensure!(!request.is_empty(), Ok(()));

        // record the maximum offset that we've seen
        self.max_recv_offset = self.max_recv_offset.max(request.end_exclusive());

        // start from the back with the assumption that most data arrives in order
        for mut idx in (0..self.slots.len()).rev() {
            let slot = &mut self.slots[idx];

            let slot::Outcome { lower, mid, upper } = slot.try_write(request);

            // if this slot was completed, we should try and unsplit with the next slot
            if slot.is_full() {
                let current_block =
                    Self::align_offset(slot.start(), Self::allocation_size(slot.start()));
                let end = slot.end();

                if let Some(next) = self.slots.get(idx + 1) {
                    let next_block =
                        Self::align_offset(next.start(), Self::allocation_size(next.start()));

                    if next.start() == end && current_block == next_block {
                        if let Some(next) = self.slots.remove(idx + 1) {
                            self.slots[idx].unsplit(next);
                        } else {
                            unsafe {
                                // Safety: we've already checked that `idx + 1` exists
                                assume!(false, "slot should be available");
                            }
                        }
                    }
                }
            }

            idx += 1;
            self.allocate_request(idx, upper);

            if let Some(mid) = mid {
                self.insert(idx, mid);
            }

            request = lower;

            if request.is_empty() {
                break;
            }
        }

        self.allocate_request(0, request);

        self.invariants();

        Ok(())
    }

    /// Advances the read and write cursors and discards any held data
    ///
    /// This can be used for copy-avoidance applications where a packet is received in order and
    /// doesn't need to be stored temporarily for future packets to unblock the stream.
    #[inline]
    pub fn skip(&mut self, len: usize) -> Result<(), ReceiveBufferError> {
        // zero-length skip is a no-op
        ensure!(len > 0, Ok(()));

        let new_start_offset = self
            .start_offset
            .checked_add(len as u64)
            .ok_or(ReceiveBufferError::OutOfRange)?;

        if let Some(final_size) = self.final_size() {
            ensure!(
                final_size >= new_start_offset,
                Err(ReceiveBufferError::InvalidFin)
            );
        }

        // record the maximum offset that we've seen
        self.max_recv_offset = self.max_recv_offset.max(new_start_offset);

        // update the current start offset
        self.start_offset = new_start_offset;

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
            if is_final_offset {
                core::mem::take(buffer)
            } else {
                buffer.split()
            }
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
            ensure!(watermark > 0, BytesMut::new());

            if watermark == buffer.len() && is_final_offset {
                return core::mem::take(buffer);
            }

            buffer.split_to(watermark)
        })
    }

    /// Pops a buffer from the front of the receive queue as long as the `transform` function returns a
    /// non-empty buffer.
    #[inline]
    fn pop_transform<F: Fn(&mut BytesMut, bool) -> BytesMut>(
        &mut self,
        transform: F,
    ) -> Option<BytesMut> {
        let slot = self.slots.front_mut()?;

        // make sure the slot has some data
        ensure!(slot.is_occupied(self.start_offset), None);

        let is_final_offset = self.final_offset == slot.end();
        let buffer = slot.data_mut();

        let out = transform(buffer, is_final_offset);

        // filter out empty buffers
        ensure!(!out.is_empty(), None);

        slot.add_start(out.len());

        if slot.should_drop() {
            // remove empty buffers
            self.slots.pop_front();
        }

        self.start_offset += out.len() as u64;

        self.invariants();

        Some(out)
    }

    /// Returns the amount of data that had already been consumed from the
    /// receive buffer.
    #[inline]
    pub fn consumed_len(&self) -> u64 {
        self.start_offset
    }

    /// Returns the total amount of contiguous received data.
    ///
    /// This includes the already consumed data as well as the data that is still
    /// buffered and available for consumption.
    #[inline]
    pub fn total_received_len(&self) -> u64 {
        self.consumed_len() + self.len() as u64
    }

    /// Resets the receive buffer.
    ///
    /// This will drop all previously received data.
    #[inline]
    pub fn reset(&mut self) {
        self.slots.clear();
        self.start_offset = Default::default();
        self.max_recv_offset = 0;
        self.final_offset = u64::MAX;
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

    #[inline]
    fn allocate_request(&mut self, mut idx: usize, mut request: Request) {
        while !request.is_empty() {
            let start = request.start();
            let mut size = Self::allocation_size(start);
            let offset = Self::align_offset(start, size);

            // if this is a fin request and the write is under the allocation size, then no need to
            // do a full allocation that doesn't end up getting used.
            if request.is_fin() {
                let size_candidate = (start - offset) as usize + request.len();
                if size_candidate < size {
                    size = size_candidate;
                }
            }

            // set the current request to the upper slot and loop
            request = self.allocate_slot(&mut idx, request, offset, size);
        }
    }

    #[inline]
    fn allocate_slot<'a>(
        &mut self,
        idx: &mut usize,
        request: Request<'a>,
        mut offset: u64,
        mut size: usize,
    ) -> Request<'a> {
        // don't allocate for data we've already consumed
        if let Some(diff) = self.start_offset.checked_sub(offset) {
            debug_assert!(
                request.start() >= self.start_offset,
                "requests should be split before allocating slots"
            );
            offset = self.start_offset;
            size -= diff as usize;
        }

        let buffer = BytesMut::with_capacity(size);

        let end = offset + size as u64;
        let mut slot = Slot::new(offset, end, buffer);

        let slot::Outcome { lower, mid, upper } = slot.try_write(request);

        debug_assert!(lower.is_empty(), "lower requests should always be empty");

        // first insert the newly-created Slot
        debug_assert!(!slot.should_drop());
        self.insert(*idx, slot);
        *idx += 1;

        // check if we have a mid-slot and insert that as well
        if let Some(mid) = mid {
            debug_assert!(!mid.should_drop());
            self.insert(*idx, mid);
            *idx += 1;
        }

        // return the upper request if we need to allocate more
        upper
    }

    /// Aligns an offset to a certain alignment size
    #[inline]
    fn align_offset(offset: u64, alignment: usize) -> u64 {
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
    #[inline]
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

    #[inline]
    fn invariants(&self) {
        if cfg!(debug_assertions) {
            let mut prev_end = self.start_offset;

            for slot in &self.slots {
                assert!(slot.start() >= prev_end, "{self:#?}");
                assert!(!slot.should_drop(), "slot range should be non-empty");
                prev_end = slot.end_allocated();
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
    fn new(buffer: &'a ReceiveBuffer) -> Self {
        Self {
            prev_end: buffer.start_offset,
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
    inner: &'a mut ReceiveBuffer,
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
