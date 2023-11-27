// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Request;
use bytes::{Buf, BufMut, BytesMut};
use core::fmt;

/// Possible states for slots in the [`ReceiveBuffer`]s queue
#[derive(PartialEq, Eq)]
pub struct Slot {
    start: u64,
    end: u64,
    data: BytesMut,
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slot")
            .field("start", &self.start)
            .field("end", &self.end())
            .field("end_allocated", &self.end_allocated())
            .field("len", &self.data.len())
            .field("capacity", &self.data.capacity())
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Outcome<'a> {
    pub lower: Request<'a>,
    pub mid: Option<Slot>,
    pub upper: Request<'a>,
}

impl Slot {
    #[inline]
    pub fn new(start: u64, end: u64, data: BytesMut) -> Self {
        let v = Self { start, end, data };
        v.invariants();
        v
    }

    #[inline(always)]
    pub fn try_write<'a>(&mut self, request: Request<'a>) -> Outcome<'a> {
        // trim off chunks lower than the start
        let (lower, request) = request.split(self.start());

        // trim off chunks we've already copied
        let (_, request) = request.split(self.end());

        // trim off chunks higher than the allocated end
        let (to_write, upper) = request.split(self.end_allocated());

        let mut mid = None;

        if let Some(to_write) = to_write.into_option() {
            if to_write.start() > self.end() {
                // find the split point between the buffers
                let len = to_write.start() - self.start();

                // create a new mid slot
                let start = to_write.start();
                let mut data = self.data.split_off(len as usize);

                // copy the data to the buffer
                to_write.write(&mut data);

                mid = Some(Self {
                    start,
                    end: self.end,
                    data,
                });
                self.end = start;
            } else {
                // copy the data to the buffer
                to_write.write(&mut self.data);
            }
        }

        self.invariants();

        Outcome { lower, mid, upper }
    }

    #[inline]
    pub fn unsplit(&mut self, next: Self) {
        debug_assert_eq!(self.end(), self.end_allocated());
        debug_assert_eq!(self.end(), next.start());
        self.data.unsplit(next.data);
        self.end = next.end;
        self.invariants();
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.end() == self.end_allocated()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline]
    pub fn is_occupied(&self, prev_offset: u64) -> bool {
        !self.is_empty() && self.start() == prev_offset
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    #[inline]
    pub fn data_mut(&mut self) -> &mut BytesMut {
        &mut self.data
    }

    #[inline]
    pub fn start(&self) -> u64 {
        self.start
    }

    #[inline]
    pub fn add_start(&mut self, len: usize) {
        self.start += len as u64;
        self.invariants()
    }

    #[inline]
    pub fn end(&self) -> u64 {
        self.start() + self.data.len() as u64
    }

    #[inline]
    pub fn end_allocated(&self) -> u64 {
        self.end
    }

    #[inline]
    pub fn skip(&mut self, len: u64) {
        // trim off the data buffer
        unsafe {
            let len = len.min(usize::MAX as u64) as usize;

            // extend the write cursor if the length extends beyond the initialized offset
            if let Some(to_advance) = len.checked_sub(self.data.len()) {
                assume!(to_advance <= self.data.remaining_mut());
                self.data.advance_mut(to_advance);
            }

            // consume `len` bytes
            let to_advance = self.data.remaining().min(len);
            self.data.advance(to_advance);
        }

        // advance the start position
        self.start += len;

        self.invariants();
    }

    /// Indicates the slot isn't capable of storing any more data and should be dropped
    #[inline]
    pub fn should_drop(&self) -> bool {
        self.start() == self.end_allocated()
    }

    #[inline]
    fn invariants(&self) {
        if cfg!(debug_assertions) {
            assert!(self.start() <= self.end(), "{:?}", self);
            assert!(self.start() <= self.end_allocated(), "{:?}", self);
            assert!(self.end() <= self.end_allocated(), "{:?}", self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::varint::VarInt;

    #[test]
    fn bytes_assumption() {
        let mut data = BytesMut::with_capacity(100);
        let half = data.split_off(50);

        // after splitting a BytesMut the capacity should also be split
        assert_eq!(data.capacity(), 50);
        assert_eq!(half.capacity(), 50);
    }

    fn slot(range: core::ops::Range<u64>, data: &[u8]) -> Slot {
        let mut buffer = BytesMut::with_capacity((range.end - range.start) as usize);
        buffer.extend_from_slice(data);
        Slot::new(range.start, range.end, buffer)
    }

    fn req(offset: u64, data: &[u8]) -> Request {
        Request::new(VarInt::new(offset).unwrap(), data, false).unwrap()
    }

    macro_rules! assert_write {
        ($slot:expr, $request:expr, $expected_slot:expr, $result:expr) => {{
            let mut slot = $slot;
            let req = $request;

            let result = slot.try_write(req);
            assert_eq!(slot, $expected_slot);
            assert_eq!(result, $result);
        }};
    }

    #[test]
    fn overlap() {
        assert_write!(
            slot(4..8, &[1]),
            req(0, &[42; 12]),
            slot(4..8, &[1, 42, 42, 42]),
            Outcome {
                lower: req(0, &[42; 4]),
                mid: None,
                upper: req(8, &[42; 4]),
            }
        );
    }

    #[test]
    fn upper() {
        assert_write!(
            slot(4..8, &[1]),
            req(8, &[42; 4]),
            slot(4..8, &[1]),
            Outcome {
                lower: req(4, &[]),
                mid: None,
                upper: req(8, &[42; 4]),
            }
        );
    }

    #[test]
    fn lower() {
        assert_write!(
            slot(4..8, &[1]),
            req(0, &[42; 4]),
            slot(4..8, &[1]),
            Outcome {
                lower: req(0, &[42; 4]),
                mid: None,
                upper: req(8, &[]),
            }
        );
    }

    #[test]
    fn mid() {
        assert_write!(
            slot(4..8, &[1]),
            req(6, &[42; 1]),
            slot(4..6, &[1]),
            Outcome {
                lower: req(4, &[]),
                mid: Some(slot(6..8, &[42])),
                upper: req(8, &[]),
            }
        );
    }

    #[test]
    fn mid_upper() {
        assert_write!(
            slot(4..8, &[1]),
            req(6, &[42; 4]),
            slot(4..6, &[1]),
            Outcome {
                lower: req(4, &[]),
                mid: Some(slot(6..8, &[42; 2])),
                upper: req(8, &[42; 2]),
            }
        );
    }
}
