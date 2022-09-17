// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::ReceiveBufferError;
use crate::varint::VarInt;
use bytes::{BufMut, BytesMut};
use core::fmt;

#[derive(PartialEq)]
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
    pub fn new(offset: VarInt, data: &'a [u8]) -> Result<Self, ReceiveBufferError> {
        offset
            .checked_add_usize(data.len())
            .ok_or(ReceiveBufferError::OutOfRange)?;
        Ok(Self {
            offset: offset.as_u64(),
            data,
        })
    }

    #[inline]
    pub fn split(self, offset: u64) -> (Self, Self) {
        let mid = offset.saturating_sub(self.offset);
        let mid = self.data.len().min(mid as _);
        let (a, b) = self.data.split_at(mid);

        let a_offset = self.offset.min(offset);
        let b_offset = self.offset.max(offset);

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
        let chunk = buffer.chunk_mut();
        unsafe {
            let len = self.data.len();
            debug_assert!(len <= chunk.len(), "{:?} <= {:?}", len, chunk.len());

            core::ptr::copy_nonoverlapping(self.data.as_ptr(), chunk.as_mut_ptr(), len);
            buffer.advance_mut(len);
        }
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
    pub fn into_option(self) -> Option<Self> {
        if self.data.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}
