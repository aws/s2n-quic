// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{reader, writer, Error, Reader},
    varint::VarInt,
};
use core::fmt;

#[derive(PartialEq, Eq)]
pub struct Request<'a> {
    offset: u64,
    data: &'a [u8],
    is_fin: bool,
}

impl fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Request")
            .field("offset", &self.offset)
            .field("len", &self.data.len())
            .field("is_fin", &self.is_fin)
            .finish()
    }
}

impl<'a> Request<'a> {
    #[inline]
    pub fn new(offset: VarInt, data: &'a [u8], is_fin: bool) -> Result<Self, Error> {
        offset
            .checked_add_usize(data.len())
            .ok_or(Error::OutOfRange)?;
        Ok(Self {
            offset: offset.as_u64(),
            data,
            is_fin,
        })
    }
}

impl Reader for Request<'_> {
    #[inline]
    fn current_offset(&self) -> VarInt {
        unsafe { VarInt::new_unchecked(self.offset) }
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        if self.is_fin {
            Some(self.current_offset() + self.data.len())
        } else {
            None
        }
    }
}

impl reader::Storage for Request<'_> {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.data.len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk<'_>, Self::Error> {
        let chunk = self.data.read_chunk(watermark)?;
        self.offset += chunk.len() as u64;
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<reader::storage::Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.track_write();
        let chunk = self.data.partial_copy_into(&mut dest)?;
        self.offset += chunk.len() as u64;
        self.offset += dest.written_len() as u64;
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.track_write();
        self.data.copy_into(&mut dest)?;
        self.offset += dest.written_len() as u64;
        Ok(())
    }
}
