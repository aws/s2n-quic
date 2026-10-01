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
    pub fn viewer(&self) -> Viewer<'_> {
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
        let mut found = false;

        // find the chunk and offset where the range starts
        for chunk in buffer.chunks.iter().skip(*chunk_index) {
            let len = chunk.len() as u64;
            let start = *stream_offset;
            let end = start + len;

            if (start..end).contains(&range.start_inclusive()) {
                offset = (range.start_inclusive().as_u64() - start) as usize;
                found = true;
                break;
            }

            *stream_offset += len;
            *chunk_index += 1;
        }

        // If the range start wasn't located (a stale/inconsistent cursor), fail closed
        // with an empty view rather than an invalid `chunk_index`/`offset`. `len == 0`
        // means it is never dereferenced. The debug_assert still catches the invariant
        // violation in test/CI; release relies on the empty view.
        if !found {
            debug_assert!(
                *chunk_index < buffer.chunks.len(),
                "range ({:?}) start could not be located in the buffer: {:?}",
                range,
                buffer.head..buffer.total_len()
            );

            return Self {
                buffer,
                chunk_index: (*chunk_index).min(buffer.chunks.len()),
                offset: 0,
                len: 0,
                is_fin: false,
            };
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

        // Checked index: a past-the-end `chunk_index` ends iteration instead of panicking.
        let Some(chunk) = view.buffer.chunks.get(view.chunk_index) else {
            view.len = 0;
            return None;
        };

        let start = view.offset;
        // reset the offset to the beginning of the next chunk
        view.offset = 0;
        // Guard against underflow: a bad `offset` (`start > chunk.len()`) ends iteration
        // instead of feeding a huge length into the slice.
        let Some(len) = chunk.len().checked_sub(start) else {
            view.len = 0;
            return None;
        };
        // make sure we don't exceed that max len
        let len = view.len.min(len);

        // compute the end of the chunk slice
        let end = start + len;
        // decrement the remaining len
        view.len -= len;
        // move to the next chunk
        view.chunk_index += 1;

        // Bounds are now provably valid (`start <= end <= chunk.len()`), so the
        // `&[u8]` `Slice` impl's `get_unchecked` fast path is safe.
        debug_assert!(start <= end && end <= chunk.len());
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
        // Zero-copy fast path only when bounds are provably valid; otherwise yield an
        // empty slice so an invalid view never triggers `get_unchecked` out-of-bounds (UB).
        if start <= end && end <= chunk.len() {
            // Safety: bounds checked above, so `start..end` is within `chunk`.
            unsafe { chunk.get_unchecked(start..end) }
        } else {
            debug_assert!(
                false,
                "ViewIter produced out-of-bounds slice bounds {start}..{end} for chunk of len {}",
                chunk.len()
            );
            &[]
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

    // Bug-condition tests: ranges whose start can't be located (isBugCondition). The fixed
    // `View::new` returns an empty view, so these assert empty-and-safe. Some shapes still
    // trip a retained `debug_assert!` in debug, so they are gated by profile (should_panic in
    // debug, empty view in release).

    /// Asserts a `View` is empty and safe: `len() == 0`, both iterators yield nothing, and
    /// encode writes nothing.
    #[cfg(not(debug_assertions))]
    fn assert_empty_view_release(view: &View) {
        assert_eq!(view.len(), VarInt::ZERO, "fixed view must be empty");

        let via_slice: Vec<u8> = view.iter::<&[u8]>().flatten().copied().collect();
        assert!(via_slice.is_empty(), "&[u8] iter must yield nothing");
        let via_bytes: Vec<Bytes> = view.iter::<Bytes>().collect();
        assert!(via_bytes.is_empty(), "Bytes iter must yield nothing");

        use s2n_codec::EncoderBuffer;
        let mut out = [0u8; 8];
        let mut encoder = EncoderBuffer::new(&mut out);
        let mut view_copy = *view;
        (&mut view_copy).encode(&mut encoder);
        assert_eq!(encoder.len(), 0, "encode of an empty view writes nothing");
    }

    /// Unlocatable ranges extending past the buffer end: empty buffer past `VarInt::MAX` (the
    /// production surface), one chunk with an out-of-range start, and empty buffer. All trip the
    /// `end_exclusive <= total_len` guard in debug; in release each yields an empty view.
    /// (Debug `should_panic` stops at the first case; release runs all three.)
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "range.end_exclusive() <= buffer.total_len()")
    )]
    #[test]
    fn bug_condition_end_past_total_len_test() {
        let one_chunk = buffer_from_chunks(&[vec![0, 1, 2]]);
        let empty = Buffer::default();
        let cases: [(&Buffer, Interval<VarInt>); 3] = [
            (&empty, (VarInt::ZERO..=VarInt::MAX).into()),
            (&one_chunk, (VarInt::from_u8(5)..=VarInt::from_u8(5)).into()),
            (&empty, (VarInt::from_u8(0)..=VarInt::from_u8(0)).into()),
        ];

        for (buffer, range) in cases {
            let mut stream_offset = buffer.head.as_u64();
            let mut chunk_index = 0usize;
            let view = View::new(buffer, range, false, &mut stream_offset, &mut chunk_index);

            #[cfg(not(debug_assertions))]
            assert_empty_view_release(&view);
            #[cfg(debug_assertions)]
            let _ = view;
        }
    }

    /// Stale cursor after release: the cursor is left on a chunk that release then dropped, so
    /// a still-live range can't be found. Debug trips the not-found guard; release yields empty.
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "start could not be located in the buffer")
    )]
    #[test]
    fn bug_condition_stale_cursor_after_release_test() {
        let mut buffer = Buffer::default();
        buffer.push(Bytes::from_static(&[0, 1, 2])); // stream offsets [0,3)
        buffer.push(Bytes::from_static(&[3, 4, 5])); // stream offsets [3,6)

        // cursor already advanced onto the second chunk
        let mut stream_offset = 3u64;
        let mut chunk_index = 1usize;

        // release the first chunk: only [3,4,5] remains, but the cursor still points at index 1
        buffer.release(VarInt::from_u8(3));
        assert_eq!(buffer.chunks.len(), 1);
        assert_eq!(buffer.chunks[0][..], [3, 4, 5]);

        // still-live range [4,5], but unlocatable from the stale cursor (skip(1) skips the
        // only remaining chunk)
        let range: Interval<VarInt> = (VarInt::from_u8(4)..=VarInt::from_u8(5)).into();

        let view = View::new(&buffer, range, false, &mut stream_offset, &mut chunk_index);

        #[cfg(not(debug_assertions))]
        assert_empty_view_release(&view);
        #[cfg(debug_assertions)]
        let _ = view;
    }

    /// Offset-past-chunk underflow: an invalid `View` (`offset > chunk.len()`) built directly,
    /// since `View::new` can't produce it. The `checked_sub` guard ends iteration safely
    /// (previously this underflowed into a silent out-of-bounds `get_unchecked` in release).
    #[test]
    fn bug_condition_offset_past_chunk_underflow_test() {
        let mut buffer = Buffer::default();
        buffer.push(Bytes::from_static(&[0, 1, 2])); // len 3

        // invalid view: chunk_index valid (0), but offset (5) exceeds chunks[0].len() (3)
        let view = View {
            buffer: &buffer,
            chunk_index: 0,
            offset: 5,
            len: 1,
            is_fin: false,
        };

        let via_bytes: Vec<Bytes> = view.iter::<Bytes>().collect();
        assert!(via_bytes.is_empty(), "Bytes iter must yield nothing");
        let via_slice: Vec<u8> = view.iter::<&[u8]>().flatten().copied().collect();
        assert!(via_slice.is_empty(), "&[u8] iter must yield nothing");
    }

    // Preservation (property-based): for every locatable range, `View::new` + `ViewIter` must
    // yield exactly the requested bytes on both slice paths and both `EncoderValue` paths, and
    // reuse the cursor across advancing ranges. These add the `Bytes`/encode and random-layout
    // coverage that `view_test`/`viewer_test` don't exercise.

    /// Builds a buffer from a list of chunk byte-slices, each pushed as its own chunk.
    fn buffer_from_chunks(chunks: &[Vec<u8>]) -> Buffer {
        let mut buffer = Buffer::default();
        for chunk in chunks {
            buffer.push(Bytes::copy_from_slice(chunk));
        }
        buffer
    }

    /// The concatenated bytes currently held by the buffer (chunk contents in order).
    fn concat_bytes(chunks: &[Vec<u8>]) -> Vec<u8> {
        chunks.iter().flat_map(|c| c.iter().copied()).collect()
    }

    /// Builds chunks from raw size bytes: each size is taken mod 8 (0 -> skipped) and each
    /// byte's value equals its absolute stream offset (mod 256), so expected bytes are
    /// trivially checkable. Keeps total length small for valid VarInt/offset arithmetic.
    fn build_chunks(sizes: &[u8]) -> Vec<Vec<u8>> {
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut next: usize = 0;
        for &sz in sizes {
            let sz = (sz % 8) as usize;
            if sz == 0 {
                continue;
            }
            chunks.push((0..sz).map(|i| ((next + i) % 256) as u8).collect());
            next += sz;
        }
        chunks
    }

    /// Bytes from the `&[u8]` path for a fresh view over `range`.
    fn view_bytes_slice(buffer: &Buffer, range: Interval<VarInt>) -> Vec<u8> {
        let mut stream_offset = buffer.head.as_u64();
        let mut chunk_index = 0usize;
        View::new(buffer, range, false, &mut stream_offset, &mut chunk_index)
            .iter::<&[u8]>()
            .flatten()
            .copied()
            .collect()
    }

    /// Bytes from the zero-copy `Bytes` path for a fresh view over `range`.
    fn view_bytes_bytes(buffer: &Buffer, range: Interval<VarInt>) -> Vec<u8> {
        let mut stream_offset = buffer.head.as_u64();
        let mut chunk_index = 0usize;
        View::new(buffer, range, false, &mut stream_offset, &mut chunk_index)
            .iter::<Bytes>()
            .flat_map(|b| b.to_vec())
            .collect()
    }

    /// Bytes from the `write_sized` `EncoderValue` path (EncoderBuffer doesn't specialize on
    /// bytes) for a fresh view over `range`.
    fn encode_write_sized(buffer: &Buffer, range: Interval<VarInt>) -> Vec<u8> {
        use s2n_codec::EncoderBuffer;
        let mut stream_offset = buffer.head.as_u64();
        let mut chunk_index = 0usize;
        let mut view = View::new(buffer, range, false, &mut stream_offset, &mut chunk_index);

        let mut out = vec![0u8; range.len()];
        let mut encoder = EncoderBuffer::new(&mut out);
        assert!(!EncoderBuffer::SPECIALIZES_BYTES);
        (&mut view).encode(&mut encoder);
        out
    }

    /// A locatable range (`len > 0`, within the buffer) and its expected buffer slice.
    /// Returns `None` when no non-empty range can be formed (empty buffer).
    fn make_locatable(
        chunk_sizes: &[u8],
        start_raw: u64,
        span_raw: u64,
    ) -> Option<(Vec<Vec<u8>>, Interval<VarInt>, core::ops::Range<usize>)> {
        let chunks = build_chunks(chunk_sizes);
        let total = chunks.iter().map(|c| c.len()).sum::<usize>() as u64;
        if total == 0 {
            return None;
        }
        // constrain start into [0, total) and span into [1, total - start]
        let start = start_raw % total;
        let max_span = total - start;
        let span = (span_raw % max_span) + 1; // in [1, max_span]
        let end = start + span;

        let range: Interval<VarInt> = (VarInt::new(start).unwrap()
            ..=VarInt::new(end - 1).unwrap())
            .into();
        Some((chunks, range, (start as usize)..(end as usize)))
    }

    /// Property: for random buffer layouts and random LOCATABLE ranges, the yielded byte
    /// sequence equals the exact requested slice of the concatenated buffer bytes, on both
    /// the `&[u8]` and zero-copy `Bytes` paths, and both `EncoderValue` paths.
    #[test]
    fn preservation_property_random_layouts_test() {
        use bolero::check;
        use bolero::generator::*;

        // up to 6 chunks, each size byte in [0,7] (0 -> skipped); start/span as u64.
        let generator = (
            produce::<Vec<u8>>().with().len(0usize..=6),
            produce::<u64>(),
            produce::<u64>(),
        );

        check!()
            .with_generator(generator)
            .for_each(|(chunk_sizes, start_raw, span_raw)| {
                let Some((chunks, range, expected_range)) =
                    make_locatable(chunk_sizes, *start_raw, *span_raw)
                else {
                    return;
                };
                let buffer = buffer_from_chunks(&chunks);
                let concat = concat_bytes(&chunks);
                let expected = concat[expected_range].to_vec();

                let via_slice = view_bytes_slice(&buffer, range);
                let via_bytes = view_bytes_bytes(&buffer, range);
                let via_write_sized = encode_write_sized(&buffer, range);

                assert_eq!(via_slice, expected, "&[u8] path, range {range:?}");
                assert_eq!(via_bytes, expected, "Bytes path, range {range:?}");
                assert_eq!(
                    via_write_sized, expected,
                    "write_sized path, range {range:?}"
                );
            });
    }

    /// Property: monotonic sequences of locatable ranges over a shared `Viewer` yield the
    /// correct bytes for each range and reuse the cursor (it never moves backwards and its
    /// tracked stream_offset stays consistent with the located chunk).
    #[test]
    fn preservation_property_monotonic_viewer_test() {
        use bolero::check;
        use bolero::generator::*;

        // a buffer layout plus a sorted list of "cut points" that define contiguous,
        // monotonically advancing ranges over the whole buffer.
        let generator = (
            produce::<Vec<u8>>().with().len(1usize..=6),
            produce::<Vec<u8>>().with().len(0usize..=6),
        );

        check!()
            .with_generator(generator)
            .for_each(|(chunk_sizes, cut_raw)| {
                let chunks = build_chunks(chunk_sizes);
                let total = chunks.iter().map(|c| c.len()).sum::<usize>();
                if total == 0 {
                    return;
                }
                let concat = concat_bytes(&chunks);
                let buffer = buffer_from_chunks(&chunks);

                // derive sorted, in-range cut points -> contiguous ranges [prev, cut)
                let mut cuts: Vec<usize> = cut_raw
                    .iter()
                    .map(|c| (*c as usize) % (total + 1))
                    .collect();
                cuts.push(total);
                cuts.sort_unstable();

                let mut viewer = buffer.viewer();
                let mut prev = 0usize;
                let mut last_offset = viewer.offset;
                let mut last_chunk_index = viewer.chunk_index;

                for cut in cuts {
                    if cut <= prev {
                        continue; // skip empty ranges (keep every range locatable, len > 0)
                    }
                    let range: Interval<VarInt> = (VarInt::new(prev as u64).unwrap()
                        ..=VarInt::new((cut - 1) as u64).unwrap())
                        .into();
                    let bytes: Vec<u8> = viewer
                        .next_view(range, false)
                        .iter::<&[u8]>()
                        .flatten()
                        .copied()
                        .collect();
                    assert_eq!(bytes, concat[prev..cut].to_vec(), "range {range:?}");

                    // cursor is reused monotonically: never moves backwards
                    assert!(viewer.offset >= last_offset);
                    assert!(viewer.chunk_index >= last_chunk_index);
                    last_offset = viewer.offset;
                    last_chunk_index = viewer.chunk_index;

                    prev = cut;
                }
            });
    }
}
