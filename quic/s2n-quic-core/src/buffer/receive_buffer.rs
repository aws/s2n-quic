// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains data structures for buffering incoming and outgoing data
//! in Quic streams.

use crate::varint::VarInt;
use alloc::collections::{vec_deque, VecDeque};
use bytes::BytesMut;

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
}

/// The default buffer size for slots that the [`ReceiveBuffer`] uses.
///
/// This value was picked as it is typically used for the default memory page size.
const MIN_BUFFER_ALLOCATION_SIZE: usize = 4096;

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
#[derive(Debug)]
pub struct ReceiveBuffer {
    slots: VecDeque<Slot>,
    start_offset: u64,
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
        // create a request
        let request = Request::new(offset, data)?;

        // trim off any data that we've already read
        let (_, mut request) = request.split(self.start_offset);

        // if the request is empty we're done
        if request.is_empty() {
            return Ok(());
        }

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
                            debug_assert!(false, "slot should be available");
                            unsafe {
                                // Safety: we've already checked that `idx + 1` exists
                                core::hint::unreachable_unchecked();
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

        self.check_consistency();

        Ok(())
    }

    /// Iterates over all of the chunks waiting to be received
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        Iter::new(self)
    }

    /// Pops a buffer from the front of the receive queue if available
    #[inline]
    pub fn pop(&mut self) -> Option<BytesMut> {
        self.pop_transform(|buffer| buffer.split())
    }

    /// Pops a buffer from the front of the receive queue, who's length is always guaranteed to be
    /// less than the provided `watermark`.
    #[inline]
    pub fn pop_watermarked(&mut self, watermark: usize) -> Option<BytesMut> {
        self.pop_transform(|buffer| {
            // make sure the buffer doesn't exceed the watermark
            let watermark = watermark.min(buffer.len());

            // if the watermark is 0 then don't needlessly increment refcounts
            if watermark == 0 {
                return BytesMut::new();
            }

            buffer.split_to(watermark)
        })
    }

    /// Pops a buffer from the front of the receive queue as long as the `transform` function returns a
    /// non-empty buffer.
    #[inline]
    fn pop_transform<F: Fn(&mut BytesMut) -> BytesMut>(
        &mut self,
        transform: F,
    ) -> Option<BytesMut> {
        let slot = self.slots.front_mut()?;

        if !slot.is_occupied(self.start_offset) {
            return None;
        }

        let buffer = slot.data_mut();

        let out = transform(buffer);

        // filter out empty buffers
        if out.is_empty() {
            return None;
        }

        slot.add_start(out.len());

        if slot.start() == slot.end_allocated() {
            // remove empty buffers
            self.slots.pop_front();
        }

        self.start_offset += out.len() as u64;

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
            let size = Self::allocation_size(request.start());
            let offset = Self::align_offset(request.start(), size);
            let buffer = BytesMut::with_capacity(size);
            let end = offset + size as u64;
            let mut slot = Slot::new(offset, end, buffer);

            let slot::Outcome { lower, mid, upper } = slot.try_write(request);

            debug_assert!(lower.is_empty(), "lower requests should always be empty");

            // first insert the newly-created Slot
            self.insert(idx, slot);
            idx += 1;

            // check if we have a mid-slot and insert that as well
            if let Some(mid) = mid {
                self.insert(idx, mid);
                idx += 1;
            }

            // set the current request to the upper slot and loop
            request = upper;
        }
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
    fn check_consistency(&self) {
        if cfg!(debug_assertions) {
            let mut prev_end = self.start_offset;

            for slot in &self.slots {
                assert!(slot.start() >= prev_end, "{self:#?}");
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

        if !slot.is_occupied(self.prev_end) {
            return None;
        }

        self.prev_end = slot.end();
        Some(slot.as_slice())
    }
}
