//! This module contains data structures for buffering incoming and outgoing data
//! in Quic streams.

use alloc::collections::VecDeque;
use bytes::BytesMut;
use s2n_quic_core::varint::VarInt;

/// Enumerates error that can occur while inserting data into the Receive Buffer
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StreamReceiveBufferError {
    /// An invalid data range was provided
    OutOfRange,
}

/// Possible states for slots in the [`StreamReceiveBuffer`]s queue
enum SlotState {
    /// We have received data for this slot
    Received(BytesMut),
    /// We have allocated a buffer for this slot, but not yet received any data inside it
    Allocated(BytesMut),
    /// We have neither allocated nor received data for this slot.
    /// The parameter is the size of the gap. Gaps are always multiples of a buffer and aligned to
    /// multiples of buffer sizes.
    Gap(u64),
}

impl core::fmt::Debug for SlotState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SlotState::Received(b) => write!(f, "SlotState::Received({})", b.len()),
            SlotState::Allocated(b) => write!(f, "SlotState::Allocated({})", b.len()),
            SlotState::Gap(gap_size) => write!(f, "SlotState::Gap({})", *gap_size),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct SlotPosition {
    /// A slots index in the slot list
    index: usize,
    /// The start offset of a slot
    offset: u64,
}

/// The default buffer size for slots that the [`StreamReceiveBuffer`] uses.
/// It not overwritten, it will always allocate buffers of this size, and fill
/// them with incoming data.
/// This limitation is documented here:
/// https://docs.rs/bytes/0.4.12/bytes/struct.Bytes.html#inline-bytes
pub const DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE: usize = 4096;

/// For buffers below this size [`BytesMut`] will inline bytes, which prevents
/// some of the mechanisms in the buffer implementation to work:
pub const MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE: usize = 4 * core::mem::size_of::<usize>();

/// Aligns an offset to a certain alignment size
fn align_offset(offset: u64, alignment: usize) -> u64 {
    (offset / (alignment as u64)) * (alignment as u64)
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#2.2
//# Endpoints MUST be able to deliver stream data to an application as an
//# ordered byte-stream.

/// `StreamReceiveBuffer` is a buffer structure for combining chunks of bytes in an
/// ordered stream, which might arrive out of order.
///
/// `StreamReceiveBuffer` will accumulate the bytes, and provide them to its users
/// once a contiguos range of bytes at the current position of the stream had
/// been accumulated.
///
/// `StreamReceiveBuffer` is optmized for minimizing memory allocations and for
/// offering it's users chunks of sizes that minimize call overhead.
/// In order to achieve this goal, `StreamReceiveBuffer` will always allocate internal
/// buffer chunks of a fixed size - which defaults to
/// [`DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE`].
///
/// If data is retrieved in smaller chunks, only the first chunk will trigger a
/// memory allocation. All other chunks can be copied into the already allocated
/// region.
///
/// When users want to consume data from the buffer, the consumable part of the
/// internal receive buffer is split off and passed back to the caller. Due to
/// this chunk beeing a view onto a reference-counted internal buffer of type
/// [`BytesMut`] this is also efficient and does not require another an
/// additional memory allocation or copy.
///
/// ## Usage
///
/// ```rust,ignore
/// use s2n_quic_transport::buffer::StreamReceiveBuffer;
///
/// let mut buffer = StreamReceiveBuffer::new();
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
pub struct StreamReceiveBuffer {
    slots: VecDeque<SlotState>,
    start_offset: u64,
    end_offset: u64,
    buffer_size: usize,
}

impl Default for StreamReceiveBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamReceiveBuffer {
    /// Creates a new `StreamReceiveBuffer` which is using the
    /// [`DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE`].
    pub fn new() -> StreamReceiveBuffer {
        StreamReceiveBuffer::with_buffer_size(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE)
    }

    /// Creates a new `StreamReceiveBuffer` using a configured buffer size.
    /// The `buffer_size` must be at least `MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE`.
    pub fn with_buffer_size(buffer_size: usize) -> StreamReceiveBuffer {
        // For simplicity reasons this simply panics if an invalid value is
        // passed. We could return a `Result` or hold the value and fail
        // allocations later on - but since this value is purely expected to be
        // a compile-time configuration value prominently showing the error to
        // help debugging seems preferred.
        if buffer_size < MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE {
            panic!(
                "Invalid buffer size. Expected at least {}",
                MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE
            );
        }

        StreamReceiveBuffer {
            slots: VecDeque::new(),
            start_offset: 0u32.into(),
            end_offset: 0u32.into(),
            buffer_size,
        }
    }

    /// Returns the amount of bytes available for reading.
    /// This equals the amount of data that is stored in contiguous fashion at
    /// the start of the buffer.
    pub fn len(&self) -> usize {
        self.report().0
    }

    /// Returns true if no bytes are available for reading
    #[allow(dead_code)] // if we have a `len`, it's good to have `is_empty`
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of bytes and chunks available for consumption
    #[inline]
    pub fn report(&self) -> (usize, usize) {
        let mut bytes = 0;
        let mut chunks = 0;
        for slot in &self.slots {
            if let SlotState::Received(b) = slot {
                bytes += b.len();
                chunks += 1;
            } else {
                // only report contiguous ready slots
                break;
            }
        }
        (bytes, chunks)
    }

    /// Allocates a buffer of the configured buffer size.
    /// This currently just allocates from the heap. In the future it could use a custom allocator
    /// or object pool.
    fn allocate_buffer(&mut self) -> Result<BytesMut, StreamReceiveBufferError> {
        let mut b = BytesMut::with_capacity(self.buffer_size);
        // Unfortunately it seems like at the current point of time we have to
        // initialize a BytesMut, in order to be able to properly split it later
        // on. split_off works based on stored size - not based on capacity. And
        // the size will be 0 if no data is stored in it yet.
        b.resize(self.buffer_size, 0);
        Ok(b)
    }

    /// Tries to merge a buffer on the given Positon to the buffer of it's left side
    fn try_merge_buffer_to_left(&mut self, mut slot_pos: SlotPosition) -> SlotPosition {
        if slot_pos.index == 0 || slot_pos.index >= self.slots.len() {
            // No buffer on the left
            return slot_pos;
        }

        let mut extracted_buf = BytesMut::new();
        let left_aligned_offset;
        let left_offset;

        if let SlotState::Received(left_buf) = &mut self.slots[slot_pos.index - 1] {
            left_offset = slot_pos.offset - left_buf.capacity() as u64;
            left_aligned_offset = align_offset(left_offset, self.buffer_size);
        } else {
            // Not a buffer
            return slot_pos;
        }

        if let SlotState::Received(right_buf) = &mut self.slots[slot_pos.index] {
            let right_aligned_offset = align_offset(slot_pos.offset, self.buffer_size);
            if right_aligned_offset != left_aligned_offset {
                // We only merge it both aligned offets are the same, which
                // means both buffers are originating from the same source
                // buffer.
                // If this property wouldn't be true, BytesMut::unsplit
                // would perform a reallocation.
                return slot_pos;
            }
            core::mem::swap(right_buf, &mut extracted_buf);
        } else {
            // Not a buffer
            return slot_pos;
        }

        // All checks succeeded
        if let SlotState::Received(left_buf) = &mut self.slots[slot_pos.index - 1] {
            left_buf.unsplit(extracted_buf);
            // Remove the unused slot and adjust the index where the
            // original (potentially middle) data is stored.
            self.slots.remove(slot_pos.index);
            slot_pos.offset = left_offset;
            slot_pos.index -= 1;
        } else {
            unreachable!("Left slot must still have a buffer");
        }

        slot_pos
    }

    /// Tries to merge a buffer on the given positon to the buffer of it's right side
    fn try_merge_buffer_to_right(&mut self, slot_pos: SlotPosition) {
        if slot_pos.index + 1 >= self.slots.len() {
            // No buffer on the right side
            return;
        }

        let mut extracted_buf = BytesMut::new();
        let left_aligned_offset;
        let right_offset;

        if let SlotState::Received(left_buf) = &mut self.slots[slot_pos.index] {
            left_aligned_offset = align_offset(slot_pos.offset, self.buffer_size);
            right_offset = slot_pos.offset + left_buf.capacity() as u64;
        } else {
            return; // Not a buffer
        }

        if let SlotState::Received(right_buf) = &mut self.slots[slot_pos.index + 1] {
            let right_aligned_offset = align_offset(right_offset, self.buffer_size);
            if right_aligned_offset != left_aligned_offset {
                // We only merge it both aligned offets are the same, which
                // means both buffers are originating from the same source
                // buffer.
                // If this property wouldn't be true, BytesMut::unsplit
                // would perform a reallocation.
                return;
            }
            core::mem::swap(right_buf, &mut extracted_buf);
        } else {
            return; // Not a buffer
        }

        // All checks succeeded
        if let SlotState::Received(left_buf) = &mut self.slots[slot_pos.index] {
            left_buf.unsplit(extracted_buf);
            // Remove the unused slot
            self.slots.remove(slot_pos.index + 1);
        } else {
            unreachable!("Left slot must still have a buffer");
        }
    }

    /// Tries to merge the buffer at the given position with adjacent buffers.
    /// `try_left` and `try_right` specify whether trying to merge in those
    /// directions should be attempted or not. We can skip attempting it if we
    /// know for sure upfront that buffers won't be mergable in this direction.
    fn try_merge_receive_buffers(
        &mut self,
        mut slot_pos: SlotPosition,
        try_left: bool,
        try_right: bool,
    ) {
        if try_left {
            // Try merging the buffer with one on the left. If this succeeds,
            // it will shift the index and offset.
            slot_pos = self.try_merge_buffer_to_left(slot_pos);
        }

        if try_right {
            self.try_merge_buffer_to_right(slot_pos);
        }
    }

    /// Allocates a new buffer which gets pushed to the end of our slot queue.
    /// The method allows to create Gaps between slots. Therefore an
    /// `aligned_offset` (which must be a mulitple of `buffer_size`) must be
    /// provided.
    fn push_back_buffer_at(&mut self, aligned_offset: u64) -> Result<(), StreamReceiveBufferError> {
        // Allocate the buffer before we do any slot
        // manipulation, so things stay consistent if it
        // would fail.
        let buffer = self.allocate_buffer()?;
        // If the aligned data_offset is not adjacent to the already covered
        // slot range we need a gap and a buffer. Otherwise we only need a buffer.
        let gap_size: u64 = aligned_offset - self.end_offset;
        if gap_size > 0u64 {
            let gap = SlotState::Gap(gap_size);
            self.slots.push_back(gap);
        }

        let slot = SlotState::Allocated(buffer);
        self.slots.push_back(slot);
        self.end_offset = Into::<u64>::into(aligned_offset) + self.buffer_size as u64;

        Ok(())
    }

    /// Allocate a buffer inside the gap.
    /// Gaps are always aligned to buffer sizes - that is why `desired_buffer_offset`
    /// needs to be aligned. All other gaps that we will create by splitting a
    /// gap will also be aligned.
    /// Returns the information about the slot which contains the allocated buffer.
    fn allocate_buffer_in_gap(
        &mut self,
        gap_index: usize,
        gap_start: u64,
        gap_size: u64,
        desired_buffer_offset: u64,
    ) -> Result<SlotPosition, StreamReceiveBufferError> {
        debug_assert!(gap_start % self.buffer_size as u64 == 0u64);
        debug_assert!(gap_size % self.buffer_size as u64 == 0u64);
        debug_assert!(desired_buffer_offset % self.buffer_size as u64 == 0u64);

        let gap_end = gap_start + gap_size;
        let new_gap_before = desired_buffer_offset - gap_start;
        let new_gap_after = gap_end - (desired_buffer_offset + self.buffer_size as u64);
        let mut buffer_index = gap_index;

        // Allocate the buffer before we do any slot
        // manipulation, so things stay consistent if it
        // would fail.
        let buffer = self.allocate_buffer()?;
        if new_gap_before > 0u64 {
            self.slots
                .insert(buffer_index, SlotState::Gap(new_gap_before));
            buffer_index += 1;
        }
        if new_gap_after > 0u64 {
            self.slots
                .insert(buffer_index + 1, SlotState::Gap(new_gap_after));
        }
        self.slots[buffer_index] = SlotState::Allocated(buffer);

        Ok(SlotPosition {
            index: buffer_index,
            offset: desired_buffer_offset,
        })
    }

    /// Ensures that a buffer is available for the given offset, and returns the
    /// index and the start offset of the slot where the given offset falls into.
    ///
    /// If no buffer has yet been allocated for the offset, new slots are appended
    /// at the end of our slot queue and a new buffer is created to cover the offset.
    ///
    /// The return index will always be either
    /// - the index of a slot which has stored existing data and contains the
    ///   offset
    /// - or the index of a slot which has a buffer ready to store new data,
    ///   and covers the offset.
    fn get_or_create_buffer_at_offset(
        &mut self,
        data_offset: u64,
    ) -> Result<SlotPosition, StreamReceiveBufferError> {
        // In order to determine whether slots already have been created for the
        // offset, we need align the offset to our buffer size. Slots will
        // always be created to cover full buffer sizes.
        let aligned_offset = align_offset(data_offset, self.buffer_size);
        if aligned_offset >= self.end_offset {
            // Create a new buffer behind all our available slots.
            // The newly created buffer will be in the last slot
            self.push_back_buffer_at(aligned_offset)?;
            return Ok(SlotPosition {
                offset: aligned_offset,
                index: self.slots.len() - 1,
            });
        }

        // We need to search the slot which will hold the data.
        // If necessary, we need to insert buffers into gaps.
        let mut current_offset = self.start_offset;

        for (index, slot) in self.slots.iter_mut().enumerate() {
            let slot_start = current_offset;
            match slot {
                SlotState::Gap(gap_size) => {
                    // If some part of our data needs to get placed into the gap
                    // we need to allocate a buffer in the gap. Otherwise we
                    // will just skip the gap.
                    let gap_size = *gap_size;
                    let gap_end = slot_start + gap_size;
                    if aligned_offset < gap_end {
                        return self.allocate_buffer_in_gap(
                            index,
                            slot_start,
                            gap_size,
                            aligned_offset,
                        );
                    }
                    current_offset += gap_size
                }
                SlotState::Received(b) | SlotState::Allocated(b) => {
                    if data_offset < slot_start + b.capacity() as u64 {
                        return Ok(SlotPosition {
                            offset: current_offset,
                            index,
                        });
                    }
                    current_offset += b.capacity() as u64;
                }
            };
        }

        unreachable!("A slot must have been available");
    }

    /// Pushes a slice at a certain offset
    pub fn write_at(
        &mut self,
        data_offset: VarInt,
        mut data: &[u8],
    ) -> Result<(), StreamReceiveBufferError> {
        // Overflow check. The total amount of received data must not exceed
        // the maximum VarInt value.
        let varint_data_len =
            VarInt::new(data.len() as u64).map_err(|_| StreamReceiveBufferError::OutOfRange)?;
        if data_offset.checked_add(varint_data_len).is_none() {
            return Err(StreamReceiveBufferError::OutOfRange);
        }

        // After the length check we continue with using u64 as a datatype,
        // since this is what the remaining internal implementation is using.
        let mut data_offset: u64 = data_offset.into();

        // Skip all data that has already been read. This will be outside of our
        // current slot indices, and we don't want to create new slots for it.
        if data_offset < self.start_offset {
            let delta = self.start_offset - data_offset;
            if delta >= data.len() as u64 {
                // Everything is already written
                return Ok(());
            } else {
                // Write the remaining data. This can not be more than usize,
                // since the caller can't pass bigger slices.
                data = &data[(delta as usize)..];
                data_offset = self.start_offset;
            }
        }

        // Write all data from the slice. This might need to go into different
        // slots - some of them already existing, others still need to be created.
        // Therefore we loop until all data has been written.
        while !data.is_empty() {
            // Search for the slot where we &data[0] needs to go
            let mut slot_pos = self.get_or_create_buffer_at_offset(data_offset)?;
            let slot = &mut self.slots[slot_pos.index];
            match slot {
                SlotState::Allocated(slot_buffer) => {
                    // Grab the buffer from the slot, so that we can later move it
                    let mut buf = core::mem::replace(slot_buffer, BytesMut::new());

                    // There is a buffer space available for our data. We need to fill
                    // it with as much data as possible, and split the Allocated
                    // slot into a Received one and 0 up to 2 Allocated ones.
                    // Since a buffer length is usize, both the start and the end
                    // gap can not exceed usize, and the following conversions are
                    // safe.
                    let start_gap: usize = (data_offset - slot_pos.offset) as usize;
                    debug_assert!(start_gap < self.buffer_size);

                    let to_copy = core::cmp::min(
                        buf.capacity() - start_gap, // Available buffer size
                        data.len(),
                    ); // Remaining data.
                    let slot_end: u64 = Into::<u64>::into(slot_pos.offset) + buf.capacity() as u64;
                    let end_gap: usize = (slot_end - Into::<u64>::into(slot_pos.offset)) as usize
                        - to_copy
                        - start_gap;

                    buf[start_gap..start_gap + to_copy].copy_from_slice(&data[..to_copy]);

                    if start_gap > 0 {
                        let mut gap_buf = buf;
                        buf = gap_buf.split_off(start_gap);
                        let gap_slot = SlotState::Allocated(gap_buf);
                        self.slots.insert(slot_pos.index, gap_slot);
                        slot_pos.index += 1; // The data slot will be at the next position
                    }

                    if end_gap > 0 {
                        let gap_buf = buf.split_off(buf.capacity() - end_gap);
                        let gap_slot = SlotState::Allocated(gap_buf);
                        self.slots.insert(slot_pos.index + 1, gap_slot);
                    }

                    self.slots[slot_pos.index] = SlotState::Received(buf);
                    self.try_merge_receive_buffers(
                        SlotPosition {
                            index: slot_pos.index,
                            offset: data_offset,
                        },
                        true,
                        true,
                    );

                    data_offset += to_copy as u64;
                    data = &data[to_copy..];
                }
                SlotState::Received(slot_buffer) => {
                    // We already have data for this offset.
                    // Therefore we can just skip copying already available data.
                    // This is mentioned by section 2.2 of the QUIC specification:

                    //# An endpoint could receive data for a stream at the same stream offset
                    //# multiple times.  Data that has already been received can be
                    //# discarded.  The data at a given offset MUST NOT change if it is sent
                    //# multiple times; an endpoint MAY treat receipt of different data at
                    //# the same offset within a stream as a connection error of type
                    //# PROTOCOL_VIOLATION.

                    // In this case we are just skipping already received data
                    // without further verification in order to save resources.
                    let stored = slot_buffer.capacity();
                    // The data in the buffer starts at slot_offset. If our
                    // data_offset is higher we need the upfront data, since we don't
                    // need to skip it.
                    let non_targetted_slot_data: usize = (data_offset - slot_pos.offset) as usize;
                    let to_skip: usize = stored - non_targetted_slot_data;
                    if to_skip > data.len() {
                        return Ok(());
                    } else {
                        // There is more data to store
                        data = &data[to_skip..];
                        data_offset += to_skip as u64;
                    }
                }
                SlotState::Gap(_) => {
                    unreachable!(
                        "get_or_create_buffer_at_offset did not return a valid buffer index"
                    );
                }
            }
        }

        Ok(())
    }

    /// Pops a buffer from the front of the receive queue if available
    #[allow(dead_code)] // This isn't used anywhere in the codebase, but is still good to have around
    pub fn pop(&mut self) -> Option<BytesMut> {
        self.pop_transform(|buffer| core::mem::replace(buffer, BytesMut::new()))
    }

    /// Pops a buffer from the front of the receive queue, who's length is always guaranteed to be
    /// less than the provided `watermark`.
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
    fn pop_transform<F: Fn(&mut BytesMut) -> BytesMut>(
        &mut self,
        transform: F,
    ) -> Option<BytesMut> {
        let slot = self.slots.front_mut()?;
        if let SlotState::Received(buffer) = slot {
            debug_assert!(
                !buffer.is_empty(),
                "a received buffer should never be empty"
            );

            let out = transform(buffer);

            if buffer.is_empty() {
                debug_assert_eq!(
                    buffer.capacity(),
                    0,
                    "buffers are always split from the allocated slot"
                );

                // remove empty buffers
                self.slots.pop_front();
            }

            // filter out empty buffers
            if out.is_empty() {
                return None;
            }

            self.start_offset += out.len() as u64;
            Some(out)
        } else {
            None
        }
    }

    /// Returns the amount of data that had already been consumed from the
    /// receive buffer.
    pub fn consumed_len(&self) -> u64 {
        self.start_offset
    }

    /// Returns the total amount of consecutive received data.
    /// This includes the already consumed data as well as the data that is still
    /// buffered and available for consumption.
    pub fn total_received_len(&self) -> u64 {
        self.consumed_len() + self.len() as u64
    }

    /// Resets the receive buffer.
    /// This will drop all previously received data.
    pub fn reset(&mut self) {
        *self = StreamReceiveBuffer::with_buffer_size(self.buffer_size)
    }
}
