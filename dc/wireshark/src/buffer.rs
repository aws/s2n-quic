// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    value::Parsed,
    wireshark_sys::{self as w, tvbuff_t},
};
use s2n_codec::{DecoderBuffer, DecoderValue};

pub struct Buffer<'a> {
    pub offset: usize,
    pub tvb: *mut tvbuff_t,
    pub packet: &'a [u8],
}

// tvb may not actually be contiguous so copy it into an owned buffer
pub fn copy_to_rust(tvb: *mut tvbuff_t) -> Vec<u8> {
    let len = unsafe { w::tvb_reported_length(tvb) as usize };
    let mut buffer: Vec<u8> = vec![0; len];
    unsafe {
        w::tvb_memcpy(tvb, buffer.as_mut_ptr() as *mut std::ffi::c_void, 0, len);
    }
    buffer
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
