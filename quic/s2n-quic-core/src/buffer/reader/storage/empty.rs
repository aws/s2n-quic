// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};

/// An empty reader [`Storage`]
#[derive(Clone, Copy, Debug, Default)]
pub struct Empty;

impl Storage for Empty {
    type Error = core::convert::Infallible;

    #[inline(always)]
    fn buffered_len(&self) -> usize {
        0
    }

    #[inline(always)]
    fn buffer_is_empty(&self) -> bool {
        true
    }

    #[inline(always)]
    fn read_chunk(&mut self, _watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        Ok(Chunk::empty())
    }

    #[inline(always)]
    fn partial_copy_into<Dest>(&mut self, _dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        Ok(Chunk::empty())
    }

    #[inline]
    fn copy_into<Dest>(&mut self, _dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        Ok(())
    }
}
