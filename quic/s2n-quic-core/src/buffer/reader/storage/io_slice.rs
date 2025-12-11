// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};
use core::{cmp::Ordering, ops::ControlFlow};

/// A vectored reader [`Storage`]
pub struct IoSlice<'a, T> {
    len: usize,
    head: &'a [u8],
    buf: &'a [T],
}

impl<'a, T> IoSlice<'a, T>
where
    T: core::ops::Deref<Target = [u8]>,
{
    #[inline]
    pub fn new(buf: &'a [T]) -> Self {
        let mut len = 0;
        let mut first_non_empty = usize::MAX;
        let mut last_non_empty = 0;

        // find the total length and the first non-empty slice
        for (idx, buf) in buf.iter().enumerate() {
            len += buf.len();
            if !buf.is_empty() {
                last_non_empty = idx;
                first_non_empty = first_non_empty.min(idx);
            }
        }

        // if there are no filled slices then return the base case
        if len == 0 {
            return Self {
                len: 0,
                head: &[],
                buf: &[],
            };
        }

        let buf = unsafe {
            // Safety: we checked above that this range is at least 1 element and is in-bounds
            buf.get_unchecked(first_non_empty..=last_non_empty)
        };

        let mut slice = Self {
            len,
            head: &[],
            buf,
        };
        slice.advance_buf_once();
        slice.invariants();
        slice
    }

    #[inline(always)]
    fn advance_buf(&mut self) {
        // keep advancing the buffer until we get a non-empty slice
        while self.head.is_empty() && !self.buf.is_empty() {
            self.advance_buf_once();
        }
    }

    #[inline(always)]
    fn advance_buf_once(&mut self) {
        unsafe {
            assume!(!self.buf.is_empty());
        }
        let (head, tail) = self.buf.split_at(1);
        self.head = &head[0][..];
        self.buf = tail;
    }

    #[inline]
    fn sub_len(&mut self, len: usize) {
        unsafe {
            assume!(self.len >= len);
        }
        self.set_len(self.len - len);
    }

    #[inline]
    fn set_len(&mut self, len: usize) {
        self.len = len;
        self.invariants();
    }

    #[inline]
    fn read_chunk_control_flow(&mut self, watermark: usize) -> ControlFlow<Chunk<'a>, Chunk<'a>> {
        // we only have one chunk left so do the happy path
        if self.buf.is_empty() {
            let len = self.head.len().min(watermark);
            let (head, tail) = self.head.split_at(len);
            self.head = tail;
            self.set_len(tail.len());
            return ControlFlow::Break(head.into());
        }

        match self.head.len().cmp(&watermark) {
            // head can be consumed and the watermark still has capacity
            Ordering::Less => {
                let head = core::mem::take(&mut self.head);
                self.advance_buf();
                self.sub_len(head.len());
                ControlFlow::Continue(head.into())
            }
            // head can be consumed and the watermark is filled
            Ordering::Equal => {
                let head = core::mem::take(&mut self.head);
                self.advance_buf();
                self.sub_len(head.len());
                ControlFlow::Break(head.into())
            }
            // head is partially consumed and the watermark is filled
            Ordering::Greater => {
                unsafe {
                    assume!(self.head.len() >= watermark);
                }
                let (head, tail) = self.head.split_at(watermark);
                self.head = tail;
                self.sub_len(head.len());
                ControlFlow::Break(head.into())
            }
        }
    }

    #[inline(always)]
    fn invariants(&self) {
        #[cfg(debug_assertions)]
        {
            // make sure the computed len matches the actual remaining len
            let mut computed = self.head.len();
            for buf in self.buf.iter() {
                computed += buf.len();
            }
            assert_eq!(self.len, computed);

            if self.head.is_empty() {
                assert!(self.buf.is_empty());
                assert_eq!(self.len, 0);
            }
        }
    }
}

impl<T> bytes::Buf for IoSlice<'_, T>
where
    T: core::ops::Deref<Target = [u8]>,
{
    #[inline]
    fn remaining(&self) -> usize {
        self.len
    }

    #[inline]
    fn chunk(&self) -> &[u8] {
        self.head
    }

    /// Advances through the vectored slices by `cnt` bytes
    #[inline]
    fn advance(&mut self, mut cnt: usize) {
        assert!(cnt <= self.len);
        let new_len = self.len - cnt;

        // special-case for when we read the entire thing
        if new_len == 0 {
            self.head = &[];
            self.buf = &[];
            self.set_len(new_len);
            return;
        }

        while cnt > 0 {
            let len = self.head.len().min(cnt);
            cnt -= len;

            if len >= self.head.len() {
                unsafe {
                    assume!(!self.buf.is_empty());
                }

                self.head = &[];
                self.advance_buf();
                continue;
            }

            self.head = &self.head[len..];
            break;
        }

        self.set_len(new_len);
    }
}

impl<T> Storage for IoSlice<'_, T>
where
    T: core::ops::Deref<Target = [u8]>,
{
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        Ok(match self.read_chunk_control_flow(watermark) {
            ControlFlow::Continue(chunk) => chunk,
            ControlFlow::Break(chunk) => chunk,
        })
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        ensure!(dest.has_remaining_capacity(), Ok(Chunk::empty()));

        loop {
            match self.read_chunk_control_flow(dest.remaining_capacity()) {
                ControlFlow::Continue(chunk) => {
                    dest.put_slice(&chunk);
                    continue;
                }
                ControlFlow::Break(chunk) => return Ok(chunk),
            }
        }
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        ensure!(dest.has_remaining_capacity(), Ok(()));

        loop {
            match self.read_chunk_control_flow(dest.remaining_capacity()) {
                ControlFlow::Continue(chunk) => {
                    dest.put_slice(&chunk);
                    continue;
                }
                ControlFlow::Break(chunk) => {
                    if !chunk.is_empty() {
                        dest.put_slice(&chunk);
                    }
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::reader::storage::Buf;

    /// ensures each storage type correctly copies multiple chunks into the destination
    #[test]
    #[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
    fn io_slice_test() {
        let mut dest = vec![];
        let mut source: Vec<Vec<u8>> = vec![];
        let mut pool = vec![];
        let mut expected = vec![];
        bolero::check!()
            .with_type::<(u16, Vec<u16>)>()
            .for_each(|(max_dest_len, source_lens)| {
                while source.len() > source_lens.len() {
                    pool.push(source.pop().unwrap());
                }

                while source.len() < source_lens.len() {
                    source.push(pool.pop().unwrap_or_default());
                }

                let mut source_len = 0;
                let mut last_chunk_idx = 0;
                let mut last_chunk_len = 0;
                let mut remaining_len = *max_dest_len as usize;
                for (idx, (len, source)) in source_lens.iter().zip(&mut source).enumerate() {
                    let fill = (idx + 1) as u8;
                    let len = *len as usize;
                    source.resize(len, fill);
                    source.fill(fill);
                    if len > 0 && remaining_len > 0 {
                        last_chunk_idx = idx;
                        last_chunk_len = len.min(remaining_len);
                    }
                    source_len += len;
                    remaining_len = remaining_len.saturating_sub(len);
                }

                let dest_len = source_len.min(*max_dest_len as usize);
                dest.resize(dest_len, 0);
                dest.fill(0);
                let dest = &mut dest[..];

                expected.resize(dest_len, 0);
                expected.fill(0);

                {
                    // don't copy the last chunk, since that should be returned
                    let source = &source[..last_chunk_idx];
                    crate::slice::vectored_copy(source, &mut [&mut expected[..]]);
                }

                let expected_chunk = source
                    .get(last_chunk_idx)
                    .map(|v| &v[..last_chunk_len])
                    .unwrap_or(&[]);

                // IoSlice implementation
                {
                    let mut source = IoSlice::new(&source);
                    let mut target = &mut dest[..];

                    let chunk = source.partial_copy_into(&mut target).unwrap();

                    assert_eq!(expected, dest);
                    assert_eq!(expected_chunk, &*chunk);
                    // reset the destination
                    dest.fill(0);
                }

                // Buf implementation
                {
                    let mut source = IoSlice::new(&source);
                    let mut source = Buf::new(&mut source);
                    let mut target = &mut dest[..];

                    let chunk = source.partial_copy_into(&mut target).unwrap();

                    assert_eq!(expected, dest);
                    assert_eq!(expected_chunk, &*chunk);
                    // reset the destination
                    dest.fill(0);
                }

                // IoSlice read_chunk
                {
                    let mut reader = IoSlice::new(&source);
                    let max_dest_len = *max_dest_len as usize;

                    let expected_chunk_len = source_lens
                        .iter()
                        .find(|len| **len > 0)
                        .copied()
                        .unwrap_or(0) as usize;
                    let expected_chunk_len = expected_chunk_len.min(max_dest_len);

                    let chunk = reader.read_chunk(max_dest_len).unwrap();

                    assert_eq!(chunk.len(), expected_chunk_len);
                }
            });
    }
}
