// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{Reader, Storage},
        writer,
    },
    varint::VarInt,
};

#[cfg(debug_assertions)]
use crate::buffer::reader::storage::Infallible;

/// Ensures [`Reader`] invariants are held as each trait function is called
pub struct Checked<'a, R>
where
    R: Reader + ?Sized,
{
    inner: &'a mut R,
    #[cfg(debug_assertions)]
    chunk: alloc::vec::Vec<u8>,
}

impl<'a, R> Checked<'a, R>
where
    R: Reader + ?Sized,
{
    #[inline(always)]
    pub fn new(inner: &'a mut R) -> Self {
        Self {
            inner,
            #[cfg(debug_assertions)]
            chunk: Default::default(),
        }
    }
}

/// Forward on to the inner reader when debug_assertions are disabled
#[cfg(not(debug_assertions))]
impl<'a, R> Storage for Checked<'a, R>
where
    R: Reader + ?Sized,
{
    type Error = R::Error;

    #[inline(always)]
    fn buffered_len(&self) -> usize {
        self.inner.buffered_len()
    }

    #[inline(always)]
    fn buffer_is_empty(&self) -> bool {
        self.inner.buffer_is_empty()
    }

    #[inline(always)]
    fn read_chunk(&mut self, watermark: usize) -> Result<super::storage::Chunk<'_>, Self::Error> {
        self.inner.read_chunk(watermark)
    }

    #[inline(always)]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<super::storage::Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.inner.partial_copy_into(dest)
    }

    #[inline(always)]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.inner.copy_into(dest)
    }
}

#[cfg(debug_assertions)]
impl<R> Storage for Checked<'_, R>
where
    R: Reader + ?Sized,
{
    type Error = R::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.inner.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.inner.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<super::storage::Chunk<'_>, Self::Error> {
        let snapshot = Snapshot::new(self.inner, watermark);

        let mut chunk = self.inner.read_chunk(watermark)?;

        // copy the returned chunk into another buffer so we can read the `inner` state
        self.chunk.clear();
        chunk.infallible_copy_into(&mut self.chunk);

        snapshot.check(self.inner, 0, self.chunk.len());

        Ok(self.chunk[..].into())
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<super::storage::Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let snapshot = Snapshot::new(self.inner, dest.remaining_capacity());
        let mut dest = dest.track_write();

        let mut chunk = self.inner.partial_copy_into(&mut dest)?;

        // copy the returned chunk into another buffer so we can read the `inner` state
        self.chunk.clear();
        chunk.infallible_copy_into(&mut self.chunk);

        snapshot.check(self.inner, dest.written_len(), self.chunk.len());

        Ok(self.chunk[..].into())
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let snapshot = Snapshot::new(self.inner, dest.remaining_capacity());
        let mut dest = dest.track_write();

        self.inner.copy_into(&mut dest)?;

        snapshot.check(self.inner, dest.written_len(), 0);

        Ok(())
    }
}

impl<R> Reader for Checked<'_, R>
where
    R: Reader + ?Sized,
{
    #[inline(always)]
    fn current_offset(&self) -> VarInt {
        self.inner.current_offset()
    }

    #[inline(always)]
    fn final_offset(&self) -> Option<VarInt> {
        self.inner.final_offset()
    }

    #[inline(always)]
    fn has_buffered_fin(&self) -> bool {
        self.inner.has_buffered_fin()
    }

    #[inline(always)]
    fn is_consumed(&self) -> bool {
        self.inner.is_consumed()
    }
}

#[cfg(debug_assertions)]
struct Snapshot {
    current_offset: VarInt,
    final_offset: Option<VarInt>,
    buffered_len: usize,
    dest_capacity: usize,
}

#[cfg(debug_assertions)]
impl Snapshot {
    #[inline]
    fn new<R: Reader + ?Sized>(reader: &R, dest_capacity: usize) -> Self {
        let current_offset = reader.current_offset();
        let final_offset = reader.final_offset();
        let buffered_len = reader.buffered_len();
        Self {
            current_offset,
            final_offset,
            buffered_len,
            dest_capacity,
        }
    }

    #[inline]
    fn check<R: Reader + ?Sized>(&self, reader: &R, dest_written_len: usize, chunk_len: usize) {
        assert!(
            chunk_len <= self.dest_capacity,
            "chunk exceeded destination"
        );

        let write_len = reader.current_offset() - self.current_offset;

        assert_eq!(
            dest_written_len as u64 + chunk_len as u64,
            write_len.as_u64(),
            "{} reader misreporting offsets",
            core::any::type_name::<R>(),
        );

        assert!(write_len <= self.buffered_len as u64);

        if self.final_offset.is_some() {
            assert_eq!(
                reader.final_offset(),
                self.final_offset,
                "{} reader changed final offset",
                core::any::type_name::<R>(),
            )
        }
    }
}
