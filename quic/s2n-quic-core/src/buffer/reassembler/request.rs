// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Error;
use crate::varint::VarInt;
use bytes::{BufMut, BytesMut};
use core::fmt;

#[derive(PartialEq, Eq)]
pub struct Request<'a> {
    offset: u64,
    data: &'a [u8],
}

impl<'a> fmt::Debug for Request<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Request")
            .field("offset", &self.offset)
            .field("len", &self.data.len())
            .finish()
    }
}

impl<'a> Request<'a> {
    #[inline]
    pub fn new(offset: VarInt, data: &'a [u8]) -> Result<Self, Error> {
        offset
            .checked_add_usize(data.len())
            .ok_or(Error::OutOfRange)?;
        Ok(Self {
            offset: offset.as_u64(),
            data,
        })
    }

    #[inline]
    pub fn split(self, offset: u64) -> (Self, Self) {
        let mid = offset.saturating_sub(self.offset);
        let mid = self.data.len().min(mid as _);
        unsafe {
            assume!(mid <= self.data.len());
        }
        let (a, b) = self.data.split_at(mid);

        let (a_offset, b_offset) = if self.offset < offset {
            (self.offset, offset)
        } else {
            (offset, self.offset)
        };

        let a = Self {
            offset: a_offset,
            data: a,
        };
        let b = Self {
            offset: b_offset,
            data: b,
        };

        (a, b)
    }

    #[inline]
    pub fn write(self, buffer: &mut BytesMut) {
        super::probe::write(self.offset, self.data.len());
        let chunk = buffer.chunk_mut();
        unsafe {
            let len = self.data.len();
            assume!(len <= chunk.len(), "{:?} <= {:?}", len, chunk.len());

            // Safety: `chunk` is always going to be uninitialized memory which is allocated through `BytesMut`.
            //         Since the receive buffer owns this allocation, it's impossible for the request to overlap
            //         with this `chunk`.
            core::ptr::copy_nonoverlapping(self.data.as_ptr(), chunk.as_mut_ptr(), len);

            assume!(buffer.len() + len <= buffer.capacity());
            buffer.advance_mut(len);
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline]
    pub fn start(&self) -> u64 {
        self.offset
    }

    #[inline]
    pub fn end_exclusive(&self) -> u64 {
        self.offset + self.len() as u64
    }

    #[inline]
    pub fn into_option(self) -> Option<Self> {
        if self.data.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}
