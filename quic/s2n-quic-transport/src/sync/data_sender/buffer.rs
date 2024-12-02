// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use alloc::collections::VecDeque;
use bytes::{Buf, Bytes};
use core::{
    convert::{TryFrom, TryInto},
    fmt,
};
use s2n_codec::{Encoder, EncoderValue};
use s2n_quic_core::{frame::FitError, interval_set::Interval, varint::VarInt};

#[derive(Debug, Default)]
pub struct Buffer {
    chunks: VecDeque<Chunk>,
    head: VarInt,
    pending_len: VarInt,
}

impl Buffer {
    /// Pushes a chunk of data into to the buffer for transmission
    pub fn push(&mut self, data: Bytes) -> Interval<VarInt> {
        debug_assert!(
            self.capacity().as_u64() >= data.len() as u64,
            "capacity should be checked before pushing"
        );
        let start = self.total_len();
        let len = VarInt::try_from(data.len()).expect("cannot send more than VarInt::MAX");
        self.pending_len += len;
        self.chunks.push_back(Chunk { data });
        self.check_integrity();

        // sub 1 so we don't overflow
        let end = start + (len - 1);

        (start..=end).into()
    }

    /// Returns the maximum capacity the buffer could ever hold
    #[inline]
    pub fn capacity(&self) -> VarInt {
        VarInt::MAX - self.total_len()
    }

    /// Clears and resets the buffer
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.head = VarInt::from_u8(0);
        self.pending_len = VarInt::from_u8(0);
        self.check_integrity();
    }

    /// Returns the total number of bytes the buffer has and is currently holding
    #[inline]
    pub fn total_len(&self) -> VarInt {
        self.head + self.pending_len
    }

    /// Returns the head or offset at which the first chunk in the buffer starts
    #[inline]
    pub fn head(&self) -> VarInt {
        self.head
    }

    /// Returns the number of bytes enqueue for transmission/retransmission
    #[inline]
    pub fn enqueued_len(&self) -> VarInt {
        self.pending_len
    }

    /// Returns true if the buffer is currently empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending_len == VarInt::from_u8(0)
    }

    /// Sets the current offset of the buffer.
    ///
    /// This should only be used in testing.
    #[cfg(test)]
    pub fn set_offset(&mut self, head: VarInt) {
        self.head = head;
    }

    /// Releases all of the chunks up to the provided offset in the buffer
    ///
    /// This method should be called after a chunk of data has been transmitted
    /// and acknowledged, as there is no longer a need for it to be buffered.
    pub fn release(&mut self, up_to: VarInt) {
        // we've already released up to this offset
        if up_to <= self.head {
            return;
        }

        debug_assert!(
            self.total_len() >= up_to,
            "cannot release more than the total len"
        );

        while let Some(mut chunk) = self.chunks.pop_front() {
            let len = VarInt::try_from(chunk.len()).unwrap();
            let start = self.head;
            let end = start + len;

            // if the end of this chunk is less than the up_to, drop it entirely
            if end <= up_to {
                self.pending_len -= len;
                self.head = end;
                continue;
            }

            // only part of the chunk has been released
            self.head = up_to;

            // compute the consumed amount for the chunk
            let consumed = self.head - start;
            self.pending_len -= consumed;
            chunk.data.advance(consumed.try_into().unwrap());

            // push the chunk back for later
            self.chunks.push_front(chunk);

            break;
        }

        self.check_integrity();
    }

    /// Releases all of the currently enqueued chunks
    pub fn release_all(&mut self) {
        self.chunks.clear();
        self.head = self.total_len();
        self.pending_len = VarInt::from_u8(0);

        self.check_integrity();
    }

    /// Returns a Viewer for the buffer
    #[inline]
    pub fn viewer(&self) -> Viewer {
        Viewer {
            buffer: self,
            offset: *self.head,
            chunk_index: 0,
        }
    }

    #[inline]
    fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            let actual: VarInt = self
                .chunks
                .iter()
                .map(|chunk| chunk.len())
                .sum::<usize>()
                .try_into()
                .unwrap();
            assert_eq!(
                actual, self.pending_len,
                "actual buffer lengths should equal `pending_len`"
            );
        }
    }
}

#[derive(Default)]
struct Chunk {
    data: Bytes,
}

impl fmt::Debug for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Chunk")
            .field("len", &self.data.len())
            .finish()
    }
}

impl core::ops::Deref for Chunk {
    type Target = Bytes;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Viewer<'a> {
    buffer: &'a Buffer,
    offset: u64,
    chunk_index: usize,
}

impl<'a> Viewer<'a> {
    /// Returns the next view in the buffer for a given range
    #[inline]
    pub fn next_view(&mut self, range: Interval<VarInt>, has_fin: bool) -> View<'a> {
        View::new(
            self.buffer,
            range,
            has_fin,
            &mut self.offset,
            &mut self.chunk_index,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    buffer: &'a Buffer,
    chunk_index: usize,
    offset: usize,
    len: usize,
    is_fin: bool,
}

impl<'a> View<'a> {
    #[inline]
    fn new(
        buffer: &'a Buffer,
        range: Interval<VarInt>,
        has_fin: bool,
        stream_offset: &mut u64,
        chunk_index: &mut usize,
    ) -> Self {
        debug_assert!(
            buffer.head <= range.start_inclusive(),
            "range ({:?}) is referring to a chunk that has already been released: {:?}",
            range,
            buffer.head..buffer.total_len()
        );

        debug_assert!(
            *stream_offset <= range.start_inclusive().as_u64(),
            "viewer is trying to go backwards from offset {:?} to {:?}",
            stream_offset,
            range.start_inclusive()
        );
        debug_assert!(range.end_exclusive() <= buffer.total_len());

        let mut offset = 0;

        // find the chunk and offset where the range starts
        for chunk in buffer.chunks.iter().skip(*chunk_index) {
            let len = chunk.len() as u64;
            let start = *stream_offset;
            let end = start + len;

            if (start..end).contains(&range.start_inclusive()) {
                offset = (range.start_inclusive().as_u64() - start) as usize;
                break;
            }

            *stream_offset += len;
            *chunk_index += 1;
        }

        debug_assert!(*chunk_index < buffer.chunks.len());

        Self {
            buffer,
            chunk_index: *chunk_index,
            offset,
            len: range.len(),
            is_fin: has_fin && range.end_inclusive() == (buffer.total_len() - 1),
        }
    }

    /// Trims off an `amount` number of bytes from the end of the view
    ///
    /// If `amount` exceeds the view `len`, Err will be returned
    #[inline]
    pub fn trim_off(&mut self, amount: usize) -> Result<(), FitError> {
        self.len = self.len.checked_sub(amount).ok_or(FitError)?;

        // trimming data off the end invalidates this
        self.is_fin &= amount == 0;

        Ok(())
    }

    /// Returns the number of bytes in the current view
    #[inline]
    pub fn len(&self) -> VarInt {
        VarInt::try_from(self.len).expect("len should always fit in a VarInt")
    }

    /// Returns `true` if the view includes the last byte in the stream
    #[inline]
    pub fn is_fin(&self) -> bool {
        self.is_fin
    }

    #[inline]
    pub fn iter<'iter, S: Slice<'iter>>(&'iter self) -> ViewIter<'iter, S> {
        ViewIter {
            view: *self,
            slice: Default::default(),
        }
    }
}

pub struct ViewIter<'a, S: Slice<'a>> {
    view: View<'a>,
    slice: core::marker::PhantomData<S>,
}

impl<'a, S: Slice<'a>> Iterator for ViewIter<'a, S> {
    type Item = S;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let view = &mut self.view;

        if view.len == 0 {
            return None;
        }

        let chunk = &view.buffer.chunks[view.chunk_index];

        let start = view.offset;
        // reset the offset to the beginning of the next chunk
        view.offset = 0;
        // compute the chunk len with the given offset
        let len = chunk.len() - start;
        // make sure we don't exceed that max len
        let len = view.len.min(len);

        // compute the end of the chunk slice
        let end = start + len;
        // decrement the remaining len
        view.len -= len;
        // move to the next chunk
        view.chunk_index += 1;

        debug_assert_eq!(chunk[start..end].len(), len);
        Some(S::from_chunk(chunk, start, end))
    }
}

/// Converts a Bytes into the implemented type
pub trait Slice<'a> {
    fn from_chunk(chunk: &'a Bytes, start: usize, end: usize) -> Self;
}

impl<'a> Slice<'a> for &'a [u8] {
    #[inline]
    fn from_chunk(chunk: &'a Bytes, start: usize, end: usize) -> Self {
        unsafe {
            debug_assert!(chunk.len() >= end);
            chunk.get_unchecked(start..end)
        }
    }
}

impl<'a> Slice<'a> for Bytes {
    #[inline]
    fn from_chunk(chunk: &'a Bytes, start: usize, end: usize) -> Self {
        chunk.slice(start..end)
    }
}

impl EncoderValue for &mut View<'_> {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        // Specialize on writing byte chunks directly instead of copying the slices
        if E::SPECIALIZES_BYTES {
            for chunk in self.iter::<Bytes>() {
                encoder.write_bytes(chunk);
            }
            return;
        }

        encoder.write_sized(self.len, |slice| {
            let mut offset = 0;
            for chunk in self.iter::<&[u8]>() {
                let len = chunk.len();
                let end = offset + len;
                unsafe {
                    // Safety: we've already checked that the slice has enough
                    // capacity with `write_sized`
                    debug_assert!(slice.len() >= end);

                    // These copies are critical to performance so use use copy_nonoverlapping
                    // directly, rather than rely on compiler optimizations to ensure we
                    // don't pay any additional costs
                    core::ptr::copy_nonoverlapping(
                        chunk.as_ptr(),
                        slice.get_unchecked_mut(offset),
                        len,
                    );
                }
                offset += len;
            }
        });
    }

    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, _encoder: &E) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn almost_full_buffer() -> Buffer {
        Buffer {
            head: VarInt::MAX - 1,
            ..Default::default()
        }
    }

    #[test]
    fn partial_release_test() {
        let mut buffer = Buffer::default();

        buffer.push(Bytes::from_static(&[0, 1, 2]));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(3));

        // trim off the first byte
        buffer.release(VarInt::from_u8(1));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(2));
        assert_eq!(buffer.chunks.len(), 1);
        assert_eq!(buffer.chunks[0][..], [1, 2]);

        // duplicate releases should be ok
        buffer.release(VarInt::from_u8(1));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(2));
        assert_eq!(buffer.chunks.len(), 1);
        assert_eq!(buffer.chunks[0][..], [1, 2]);
    }

    #[test]
    fn full_release_test() {
        let mut buffer = Buffer::default();

        buffer.push(Bytes::from_static(&[0, 1, 2]));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(3));

        // trim off all bytes
        buffer.release(VarInt::from_u8(3));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(0));
        assert!(buffer.chunks.is_empty());

        // duplicate releases should be ok
        buffer.release(VarInt::from_u8(3));
        assert_eq!(buffer.total_len(), VarInt::from_u8(3));
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(0));
        assert!(buffer.chunks.is_empty());
    }

    #[test]
    fn varint_max_test() {
        let mut buffer = almost_full_buffer();

        buffer.push(Bytes::from_static(&[0]));

        buffer.release(VarInt::MAX);

        assert_eq!(buffer.total_len(), VarInt::MAX);
        assert_eq!(buffer.enqueued_len(), VarInt::from_u8(0));
        assert!(buffer.chunks.is_empty());
    }

    #[test]
    #[should_panic]
    fn varint_overflow_test() {
        let mut buffer = almost_full_buffer();

        // pushing 2 bytes will exceed the capacity and panic
        buffer.push(Bytes::from_static(&[0, 1]));
    }

    fn check_view(buffer: &Buffer, interval: Interval<u64>, expected: &[u8]) {
        let interval = (VarInt::new(interval.start_inclusive()).unwrap()
            ..=VarInt::new(interval.end_inclusive()).unwrap())
            .into();
        let actual: Vec<u8> = View::new(buffer, interval, false, &mut buffer.head.as_u64(), &mut 0)
            .iter::<&[u8]>()
            .flatten()
            .copied()
            .collect();
        assert_eq!(actual, expected);
    }

    fn check_viewer(viewer: &mut Viewer, interval: Interval<u64>, expected: &[u8]) {
        let interval = (VarInt::new(interval.start_inclusive()).unwrap()
            ..=VarInt::new(interval.end_inclusive()).unwrap())
            .into();
        let actual: Vec<u8> = viewer
            .next_view(interval, false)
            .iter::<&[u8]>()
            .flatten()
            .copied()
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn view_test() {
        let mut buffer = Buffer::default();

        buffer.push(Bytes::from_static(&[0, 1, 2]));
        buffer.push(Bytes::from_static(&[3, 4, 5]));

        check_view(&buffer, (0..1).into(), &[0]);
        check_view(&buffer, (0..2).into(), &[0, 1]);
        check_view(&buffer, (0..3).into(), &[0, 1, 2]);
        check_view(&buffer, (0..4).into(), &[0, 1, 2, 3]);
        check_view(&buffer, (0..5).into(), &[0, 1, 2, 3, 4]);
        check_view(&buffer, (0..6).into(), &[0, 1, 2, 3, 4, 5]);
        check_view(&buffer, (1..6).into(), &[1, 2, 3, 4, 5]);
        check_view(&buffer, (2..6).into(), &[2, 3, 4, 5]);
        check_view(&buffer, (3..6).into(), &[3, 4, 5]);
        check_view(&buffer, (4..6).into(), &[4, 5]);
        check_view(&buffer, (5..6).into(), &[5]);
    }

    #[test]
    fn viewer_test() {
        let mut buffer = Buffer::default();

        buffer.push(Bytes::from_static(&[0, 1, 2]));
        buffer.push(Bytes::from_static(&[3, 4, 5]));

        let mut viewer = buffer.viewer();

        check_viewer(&mut viewer, (0..1).into(), &[0]);
        check_viewer(&mut viewer, (2..4).into(), &[2, 3]);
        check_viewer(&mut viewer, (5..6).into(), &[5]);
    }
}
