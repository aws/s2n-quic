// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::ReceiveBufferError;
use crate::varint::VarInt;
use bytes::{BufMut, BytesMut};
use core::fmt;

#[derive(PartialEq, Eq)]
pub struct Request<'a> {
    offset: u64,
    data: &'a [u8],
    is_fin: bool,
}

impl<'a> fmt::Debug for Request<'a> {
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
    pub fn new(offset: VarInt, data: &'a [u8], is_fin: bool) -> Result<Self, ReceiveBufferError> {
        offset
            .checked_add_usize(data.len())
            .ok_or(ReceiveBufferError::OutOfRange)?;
        Ok(Self {
            offset: offset.as_u64(),
            data,
            is_fin,
        })
    }

    #[inline]
    pub fn split(self, offset: u64) -> (Self, Self) {
        let mid = offset.saturating_sub(self.offset);
        let mid = self.data.len().min(mid as _);
        let (a, b) = self.data.split_at(mid);

        let a_offset = self.offset.min(offset);
        let b_offset = self.offset.max(offset);

        let mut a = Self {
            offset: a_offset,
            data: a,
            is_fin: false,
        };
        let mut b = Self {
            offset: b_offset,
            data: b,
            is_fin: false,
        };

        if self.is_fin {
            let fin_offset = self.end_exclusive();

            if b.offset == fin_offset {
                a.is_fin = true;
            } else if b.end_exclusive() == fin_offset {
                b.is_fin = true;
            }
        }

        (a, b)
    }

    #[inline]
    pub fn write(self, buffer: &mut BytesMut) {
        let chunk = buffer.chunk_mut();
        unsafe {
            let len = self.data.len();
            debug_assert!(len <= chunk.len(), "{:?} <= {:?}", len, chunk.len());

            // Safety: `chunk` is always going to be uninitialized memory which is allocated through `BytesMut`.
            //         Since the receive buffer owns this allocation, it's impossible for the request to overlap
            //         with this `chunk`.
            core::ptr::copy_nonoverlapping(self.data.as_ptr(), chunk.as_mut_ptr(), len);
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
    pub fn is_fin(&self) -> bool {
        self.is_fin
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
