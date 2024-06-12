// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(test, allow(dead_code))]

use crate::{value::Parsed, wireshark_sys::tvbuff_t};
use s2n_codec::{DecoderBuffer, DecoderValue};

pub struct Buffer<'a> {
    pub offset: usize,
    pub tvb: *mut tvbuff_t,
    pub packet: &'a [u8],
}

impl<'a> Buffer<'a> {
    // SAFETY: packet must come from `tvb`.
    pub unsafe fn new(tvb: *mut tvbuff_t, packet: &'a [u8]) -> Buffer<'a> {
        Buffer {
            offset: 0,
            tvb,
            packet,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.packet.len()
    }

    pub fn consume<'t, T: DecoderValue<'t>>(&'t mut self) -> Option<Parsed<T>> {
        let start = self.offset;
        let decoder = DecoderBuffer::new(self.packet.get(self.offset..)?);

        let before = decoder.len();
        let (value, tail) = decoder.decode::<T>().ok()?;
        let after = tail.len();
        let len = before - after;

        self.offset += len;
        Some(Parsed {
            offset: start,
            len,
            value,
        })
    }

    pub fn consume_bytes<L: TryInto<usize>>(&mut self, len: L) -> Option<Parsed<&'a [u8]>> {
        let len = len.try_into().ok()?;
        let start = self.offset;
        let bytes = self.packet.get(self.offset..)?.get(..len)?;
        self.offset += len;
        debug_assert_eq!(len, bytes.len());
        Some(Parsed {
            offset: start,
            len,
            value: bytes,
        })
    }
}
