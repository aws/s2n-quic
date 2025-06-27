// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    buffer::{
        reader::{testing::Fallible, Storage as _},
        writer::Storage as _,
    },
    stream::testing::Data,
    varint::{VarInt, MAX_VARINT_VALUE},
};
use bolero::{check, generator::*};

#[derive(Copy, Clone, Debug, TypeGenerator)]
enum Op {
    Write {
        offset: VarInt,
        #[generator(0..=Data::MAX_CHUNK_LEN)]
        len: usize,
        is_fin: bool,
        is_error: bool,
    },
    Pop {
        watermark: Option<u16>,
    },
    Skip {
        len: VarInt,
    },
}

#[derive(Debug)]
struct Model {
    buffer: Reassembler,
    recv: Data,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            buffer: Reassembler::new(),
            recv: Data::new(u64::MAX),
        }
    }
}

impl Model {
    fn apply_all(&mut self, ops: &[Op]) {
        for op in ops {
            self.apply(op);
        }
    }

    fn apply(&mut self, op: &Op) {
        let Self { buffer, recv } = self;

        match *op {
            Op::Write {
                offset,
                len,
                is_fin,
                is_error,
            } => {
                let mut send = if is_fin {
                    Data::new(offset.as_u64() + len as u64)
                } else {
                    Data::new(u64::MAX)
                };
                send.seek_forward(offset.as_u64());
                let mut send = send.with_read_limit(len);

                // inject errors
                if is_error {
                    let mut send = Fallible::new(&mut send).with_error(());
                    let _ = buffer.write_reader(&mut send);
                } else {
                    let _ = buffer.write_reader(&mut send);
                }
            }
            Op::Pop { watermark } => {
                if let Some(watermark) = watermark {
                    let mut recv = recv.with_write_limit(watermark as _);
                    let _ = buffer.copy_into(&mut recv);
                } else {
                    let _ = buffer.copy_into(recv);
                }
            }
            Op::Skip { len } => {
                let consumed_len = buffer.consumed_len();
                if buffer.skip(len).is_ok() {
                    let new_consumed_len = buffer.consumed_len();
                    assert_eq!(new_consumed_len, consumed_len + len.as_u64());
                    recv.seek_forward(len.as_u64());
                }
            }
        }
    }

    fn finish(&mut self) {
        // make sure a cleared buffer is the same as a new one
        self.buffer.reset();
        assert_eq!(self.buffer, Reassembler::new());
    }
}

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn model_test() {
    check!().with_type::<Vec<Op>>().for_each(|ops| {
        let mut model = Model::default();
        model.apply_all(ops);
        model.finish();
    })
}

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn write_and_pop() {
    let mut buffer = Reassembler::new();
    let mut offset = VarInt::default();
    let chunk = Data::send_one_at(0, 9000);
    let mut popped_bytes = 0;
    for _ in 0..10000 {
        buffer.write_at(offset, &chunk).unwrap();
        offset += chunk.len();
        while let Some(chunk) = buffer.pop() {
            popped_bytes += chunk.len();
        }
    }
    assert_eq!(offset, popped_bytes);
}

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn write_and_copy_into_buf() {
    use crate::buffer::reader::Storage;

    let mut buffer = Reassembler::new();
    let mut offset = VarInt::default();
    let mut output: Vec<u8> = vec![];
    for len in 0..10000 {
        dbg!(len, offset);
        let chunk = Data::send_one_at(offset.as_u64(), len);
        buffer.write_at(offset, &chunk).unwrap();
        offset += chunk.len();
        buffer.copy_into(&mut output).unwrap();
        assert_eq!(output.len(), chunk.len());
        assert_eq!(&output[..], &chunk[..]);
        output.clear();
    }
}

fn new_receive_buffer() -> Reassembler {
    let buffer = Reassembler::new();
    assert_eq!(buffer.len(), 0);
    buffer
}

#[test]
fn gap_replacement_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert_eq!(None, buffer.pop());
}

#[test]
fn gap_subset_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(2u32.into(), &[2]).is_ok());
    assert_eq!(0, buffer.len());
    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_beginning_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_end_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_overlap_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(2u32.into(), &[2]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1, 2, 3]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_later_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(2u32.into(), &[2]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_multiple_subset_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_partial_overlay_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2]).is_ok());
    assert_eq!(3, buffer.len());

    assert_eq!(&[0u8, 1, 2], &buffer.pop().unwrap()[..]);
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3]).is_ok());
    assert_eq!(5, buffer.len());

    assert_eq!(&[3u8, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn gap_partial_stale_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1]).is_ok());
    assert_eq!(2, buffer.len());

    assert_eq!(&[0u8, 1], &buffer.pop().unwrap()[..]);
    assert_eq!(0, buffer.len());

    assert_eq!(None, buffer.pop());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert_eq!(&[2, 3, 4, 5], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn chunk_duplicate_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len(), "{buffer:#?}");

    assert!(buffer.write_at(0u32.into(), &[10, 11, 12, 13]).is_ok()); // exact match
    assert!(buffer.write_at(0u32.into(), &[20, 21]).is_ok()); // beginning
    assert!(buffer.write_at(1u32.into(), &[31, 32]).is_ok()); // middle
    assert!(buffer.write_at(2u32.into(), &[42, 43]).is_ok()); // end

    assert_eq!(4, buffer.len());
    assert_eq!(&[0, 1, 2, 3], &buffer.pop().unwrap()[..]);
}

#[test]
fn chunk_superset_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0]).is_ok());
    assert_eq!(1, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1]).is_ok());
    assert_eq!(2, buffer.len());

    assert!(buffer.write_at(2u32.into(), &[2]).is_ok());
    assert_eq!(3, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3]).is_ok());
    assert_eq!(4, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3, 4]).is_ok());
    assert_eq!(5, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn chunk_stale_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(4, buffer.len());

    // ignore stale data
    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert_eq!(&[4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn chunk_partial_smaller_before_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6, 7]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(2u32.into(), &[2, 3, 4, 5]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 1]).is_ok());
    assert_eq!(8, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn chunk_partial_larger_before_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(4u32.into(), &[4, 5, 6]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1, 2, 3, 4, 5]).is_ok());
    assert_eq!(0, buffer.len());

    assert!(buffer.write_at(0u32.into(), &[0, 2]).is_ok());
    assert_eq!(7, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6], &buffer.pop().unwrap()[..]);
}

#[test]
fn chunk_partial_smaller_after_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0, 1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert!(buffer.write_at(3u32.into(), &[3, 4]).is_ok());
    assert_eq!(5, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3, 4], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
fn chunk_partial_larger_after_test() {
    let mut buffer = new_receive_buffer();

    assert!(buffer.write_at(0u32.into(), &[0, 1]).is_ok());
    assert_eq!(2, buffer.len());

    assert!(buffer.write_at(1u32.into(), &[1, 2, 3]).is_ok());
    assert_eq!(4, buffer.len());

    assert_eq!(&[0u8, 1, 2, 3], &buffer.pop().unwrap()[..]);
    assert!(buffer.is_empty());
}

#[test]
#[allow(clippy::cognitive_complexity)] // several operations are needed to get the buffer in the desired state
fn write_and_read_buffer() {
    let mut buf = Reassembler::new();

    assert_eq!(0, buf.len());
    assert!(buf.is_empty());
    assert_eq!(None, buf.pop());

    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(4, buf.len());
    assert!(!buf.is_empty());
    assert_eq!(&[0u8, 1, 2, 3], &*buf.pop().unwrap());
    assert_eq!(0, buf.len());
    assert!(buf.is_empty());
    assert_eq!(None, buf.pop());

    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    buf.write_at(2u32.into(), &[2, 3, 4, 5, 6]).unwrap();
    assert_eq!(3, buf.len());
    assert_eq!(&[4, 5, 6], &*buf.pop().unwrap());

    // Create a gap
    buf.write_at(10u32.into(), &[10, 11, 12]).unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    // Fill the gap
    buf.write_at(7u32.into(), &[7, 8, 9]).unwrap();
    assert_eq!(6, buf.len());
    assert_eq!(&[7, 8, 9, 10, 11, 12], &*buf.pop().unwrap());
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    // Create another gap
    buf.write_at(13u32.into(), &[13]).unwrap();
    assert_eq!(1, buf.len());

    buf.write_at(16u32.into(), &[16, 17, 18]).unwrap();
    assert_eq!(1, buf.len());

    // Overfill the gap
    buf.write_at(12u32.into(), &[12, 13, 14, 15, 16, 17, 18, 19, 20])
        .unwrap();
    assert_eq!(8, buf.len());
    assert_eq!(&[13, 14, 15, 16, 17, 18, 19, 20], &*buf.pop().unwrap());
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());
}

#[test]
fn fill_preallocated_gaps() {
    let mut buf: Reassembler = Reassembler::new();

    buf.write_at((MIN_BUFFER_ALLOCATION_SIZE as u32 + 2).into(), &[42, 45])
        .unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    let mut first_chunk = [0u8; MIN_BUFFER_ALLOCATION_SIZE - 2];
    for (idx, b) in first_chunk.iter_mut().enumerate() {
        *b = idx as u8;
    }

    buf.write_at(0u32.into(), &first_chunk).unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE - 2, buf.len());

    // Overfill gap
    buf.write_at(
        (MIN_BUFFER_ALLOCATION_SIZE as u32 - 4).into(),
        &[91, 92, 93, 94, 95, 96, 97, 98, 99],
    )
    .unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE + 5, buf.len());

    let chunk = buf.pop().unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE, chunk.len());
    let mut expected = 0;
    for b in &chunk[..MIN_BUFFER_ALLOCATION_SIZE - 2] {
        assert_eq!(expected, *b);
        expected = expected.wrapping_add(1);
    }
    assert_eq!(93, chunk[MIN_BUFFER_ALLOCATION_SIZE - 2]);
    assert_eq!(94, chunk[MIN_BUFFER_ALLOCATION_SIZE - 1]);

    let chunk = buf.pop().unwrap();
    assert_eq!(5, chunk.len());
    assert_eq!(95, chunk[0]);
    assert_eq!(96, chunk[1]);
    assert_eq!(42, chunk[2]);
    assert_eq!(45, chunk[3]);
    assert_eq!(99, chunk[4]);

    assert_eq!(None, buf.pop());
}

#[test]
fn create_and_fill_large_gaps() {
    let mut buf = Reassembler::new();
    // This creates 3 full buffer gaps of full allocation ranges
    buf.write_at(
        (MIN_BUFFER_ALLOCATION_SIZE as u32 * 3 + 2).into(),
        &[7, 8, 9],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Insert something in the middle, which still leaves us with 2 gaps
    buf.write_at(
        (MIN_BUFFER_ALLOCATION_SIZE as u32 + 1).into(),
        &[10, 11, 12],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Close the last gap
    buf.write_at(
        (MIN_BUFFER_ALLOCATION_SIZE as u32 * 2 + 1).into(),
        &[14, 15, 16],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Close the first gap
    buf.write_at(4u32.into(), &[4, 5, 6, 7]).unwrap();
    assert_eq!(0, buf.len());

    // Now fill all the preallocated chunks with data
    let data = [0u8; MIN_BUFFER_ALLOCATION_SIZE * 3 + 20];
    buf.write_at(0u32.into(), &data).unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE * 3 + 20, buf.len());

    fn assert_zero_content(slice: &[u8]) {
        for b in slice {
            assert_eq!(0, *b);
        }
    }

    // Check the first allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..4]);
    assert_eq!(&[4, 5, 6, 7], &chunk[4..8]);
    assert_zero_content(&chunk[8..MIN_BUFFER_ALLOCATION_SIZE]);

    // Check the second allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..1]);
    assert_eq!(&[10, 11, 12], &chunk[1..4]);
    assert_zero_content(&chunk[4..MIN_BUFFER_ALLOCATION_SIZE]);

    // Check the third allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(MIN_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..1]);
    assert_eq!(&[14, 15, 16], &chunk[1..4]);
    assert_zero_content(&chunk[4..MIN_BUFFER_ALLOCATION_SIZE]);

    // Check the forth allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(20, chunk.len());
    assert_zero_content(&chunk[..2]);
    assert_eq!(&[7, 8, 9], &chunk[2..5]);
    assert_zero_content(&chunk[5..20]);

    // No more data should be available
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());
}

#[test]
fn ignore_already_consumed_data() {
    let mut buf = Reassembler::new();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(0, buf.consumed_len());
    assert_eq!(&[0u8, 1, 2, 3], &*buf.pop().unwrap());
    assert_eq!(4, buf.consumed_len());

    // Add partially consumed data
    buf.write_at(2u32.into(), &[2, 3, 4]).unwrap();
    assert_eq!(4, buf.consumed_len());
    assert_eq!(&[4], &*buf.pop().unwrap());
    assert_eq!(5, buf.consumed_len());

    // Add data which had been fully consumed before
    for start_offset in 0..3 {
        buf.write_at((start_offset as u32).into(), &[0, 1, 2, 3][start_offset..])
            .unwrap();
        assert_eq!(0, buf.len());
        assert_eq!(None, buf.pop());
        assert_eq!(5, buf.consumed_len());
    }
}

#[test]
fn merge_right() {
    let mut buf = Reassembler::new();
    buf.write_at(4u32.into(), &[4, 5, 6]).unwrap();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(7, buf.len());
    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6], &*buf.pop().unwrap());
}

#[test]
fn merge_left() {
    let mut buf = Reassembler::new();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    buf.write_at(4u32.into(), &[4, 5, 6]).unwrap();
    assert_eq!(7, buf.len());
    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6], &*buf.pop().unwrap());
}

#[test]
fn merge_both_sides() {
    let mut buf = Reassembler::new();
    // Create gaps on all sides, and merge them later
    buf.write_at(4u32.into(), &[4, 5]).unwrap();
    buf.write_at(8u32.into(), &[8, 9]).unwrap();
    buf.write_at(6u32.into(), &[6, 7]).unwrap();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(10, buf.len());
    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9], &*buf.pop().unwrap());
}

#[test]
fn do_not_merge_across_allocations_right() {
    let mut buf = Reassembler::new();
    let mut data_left = [0u8; MIN_BUFFER_ALLOCATION_SIZE];
    let mut data_right = [0u8; MIN_BUFFER_ALLOCATION_SIZE];

    data_left[MIN_BUFFER_ALLOCATION_SIZE - 1] = 13;
    data_right[0] = 72;

    buf.write_at((MIN_BUFFER_ALLOCATION_SIZE as u32).into(), &data_right[..])
        .unwrap();
    buf.write_at(0u32.into(), &data_left[..]).unwrap();
    assert_eq!(2 * MIN_BUFFER_ALLOCATION_SIZE, buf.len());

    assert_eq!(&data_left[..], &*buf.pop().unwrap());
    assert_eq!(&data_right[..], &*buf.pop().unwrap());
}

#[test]
fn do_not_merge_across_allocations_left() {
    let mut buf = Reassembler::new();
    let mut data_left = [0u8; MIN_BUFFER_ALLOCATION_SIZE];
    let mut data_right = [0u8; MIN_BUFFER_ALLOCATION_SIZE];

    data_left[MIN_BUFFER_ALLOCATION_SIZE - 1] = 13;
    data_right[0] = 72;

    buf.write_at(0u32.into(), &data_left[..]).unwrap();
    buf.write_at((MIN_BUFFER_ALLOCATION_SIZE as u32).into(), &data_right[..])
        .unwrap();
    assert_eq!(2 * MIN_BUFFER_ALLOCATION_SIZE, buf.len());

    assert_eq!(&data_left[..], &*buf.pop().unwrap());
    assert_eq!(&data_right[..], &*buf.pop().unwrap());
}

#[test]
fn reset_buffer() {
    let mut buf = Reassembler::new();
    buf.write_at(2u32.into(), &[2, 3]).unwrap();
    buf.write_at(0u32.into(), &[0, 1]).unwrap();
    assert_eq!(4, buf.len());
    buf.pop().unwrap();
    assert_eq!(0, buf.len());

    buf.write_at(4u32.into(), &[4, 5]).unwrap();
    assert_eq!(2, buf.len());
    assert_eq!(4, buf.consumed_len());

    buf.reset();
    assert_eq!(0, buf.len());
    assert_eq!(0, buf.consumed_len());
}

#[test]
fn write_data_till_end_of_varint() {
    let mut buffer = new_receive_buffer();

    for nr_bytes in 0..64 * 2 + 1 {
        let data = vec![0u8; nr_bytes];
        assert!(buffer
            .write_at(
                VarInt::new(MAX_VARINT_VALUE - nr_bytes as u64).unwrap(),
                &data[..]
            )
            .is_ok());
    }
}

#[test]
fn fail_to_push_out_of_bounds_data() {
    let mut buffer = new_receive_buffer();

    for nr_bytes in 0..64 * 2 + 1 {
        let data = vec![0u8; nr_bytes + 1];
        assert_eq!(
            Err(Error::OutOfRange),
            buffer.write_at(
                VarInt::new(MAX_VARINT_VALUE - nr_bytes as u64).unwrap(),
                &data[..]
            )
        );
    }
}

#[test]
#[cfg_attr(miri, ignore)] // miri fails because the slice points to invalid memory
#[cfg(target_pointer_width = "64")]
fn fail_to_push_out_of_bounds_data_with_long_buffer() {
    let mut buffer = Reassembler::new();

    // Overflow the allowed buffers by size 1. This uses an invalid memory
    // reference, due to not wanting to allocate too much memory. This is
    // ok in order to make sure that the overflow check works.
    // fake_data is based on a valid base pointer, in order to give us a
    // bit more certainty that the test won't fail for other reasons.
    let data = [0u8; 32];
    let fake_data = unsafe {
        core::slice::from_raw_parts(&data as *const u8, MAX_VARINT_VALUE as usize - 20 + 1)
    };

    for _ in 0..64 * 2 + 1 {
        assert_eq!(
            Err(Error::OutOfRange),
            buffer.write_at(20u32.into(), fake_data)
        );
    }
}

#[test]
fn pop_watermarked_test() {
    let mut buffer = Reassembler::new();

    assert_eq!(
        None,
        buffer.pop_watermarked(1),
        "an empty buffer should always return none"
    );

    buffer
        .write_at(0u32.into(), &[0, 1, 2, 3, 4, 5, 6, 7])
        .unwrap();

    assert_eq!(
        None,
        buffer.pop_watermarked(0),
        "A 0 sized watermark should not pop the buffer"
    );

    assert_eq!(
        Some(bytes::BytesMut::from(&[0, 1, 2][..])),
        buffer.pop_watermarked(3),
        "the watermark should split off a piece of the buffer",
    );

    assert_eq!(
        Some(bytes::BytesMut::from(&[3][..])),
        buffer.pop_watermarked(1),
        "the watermark should split off another piece of the buffer",
    );

    assert_eq!(
        Some(bytes::BytesMut::from(&[4, 5, 6, 7][..])),
        buffer.pop_watermarked(100),
        "popping with a high watermark should remove the remaining chunk",
    );

    assert_eq!(
        None,
        buffer.pop_watermarked(1),
        "the receive buffer should be empty after splitting"
    );
}

// these are various chunk sizes that should trigger different behavior
const INTERESTING_CHUNK_SIZES: &[u32] = &[4, 4095, 4096, 4097];

#[test]
#[cfg_attr(miri, ignore)] // this allocates too many bytes for miri
fn write_start_fin_test() {
    for size in INTERESTING_CHUNK_SIZES.iter().copied() {
        for pre_empty_fin in [false, true] {
            dbg!(size, pre_empty_fin);

            let bytes: Vec<u8> = Iterator::map(0..size, |v| v as u8).collect();
            let mut buffer = Reassembler::new();

            // write the fin offset first
            if pre_empty_fin {
                buffer.write_at_fin(size.into(), &[]).unwrap();
                buffer.write_at(0u32.into(), &bytes).unwrap();
            } else {
                buffer.write_at_fin(0u32.into(), &bytes).unwrap();
            }

            let expected = if size > 4096 {
                let chunk = buffer.pop().expect("buffer should pop chunk");
                assert_eq!(chunk.len(), 4096);
                let (first, rest) = bytes.split_at(4096);
                assert_eq!(&chunk[..], first);
                rest.to_vec()
            } else {
                bytes
            };

            let chunk = buffer.pop().expect("buffer should pop final chunk");

            assert_eq!(
                chunk.capacity(),
                expected.len(),
                "final chunk should only allocate what is needed"
            );

            assert_eq!(&chunk[..], &expected);
        }
    }
}

#[test]
#[cfg_attr(miri, ignore)] // this allocates too many bytes for miri
fn write_partial_fin_test() {
    for partial_size in INTERESTING_CHUNK_SIZES.iter().copied() {
        for fin_size in INTERESTING_CHUNK_SIZES.iter().copied() {
            for reverse in [false, true] {
                dbg!(partial_size, fin_size, reverse);

                let partial_bytes: Vec<u8> = Iterator::map(0..partial_size, |v| v as u8).collect();
                let fin_bytes: Vec<u8> = Iterator::map(0..fin_size, |v| v as u8).collect();

                let mut buffer = Reassembler::new();
                assert!(!buffer.is_writing_complete());
                assert!(!buffer.is_reading_complete());

                let mut oracle = Reassembler::new();

                let mut requests = vec![
                    (0u32, &partial_bytes, false),
                    (partial_size, &fin_bytes, true),
                ];

                if reverse {
                    requests.reverse();
                }

                for (offset, data, is_fin) in requests {
                    oracle.write_at(offset.into(), data).unwrap();
                    if is_fin {
                        buffer.write_at_fin(offset.into(), data).unwrap()
                    } else {
                        buffer.write_at(offset.into(), data).unwrap()
                    }
                }

                assert!(buffer.is_writing_complete());

                let mut results = vec![];

                for buf in [&mut buffer, &mut oracle] {
                    let mut chunks = vec![];
                    let mut actual_len = 0;

                    // look at how many bytes we actually allocated
                    let allocated_len: u64 = buf
                        .slots
                        .iter()
                        .map(|slot| slot.end_allocated() - slot.start())
                        .sum();

                    while let Some(chunk) = buf.pop() {
                        actual_len += chunk.len();
                        chunks.push(chunk);
                    }

                    assert_eq!(
                        (partial_size + fin_size) as usize,
                        actual_len,
                        "the number of received bytes should match"
                    );

                    results.push((chunks, actual_len, allocated_len));
                }

                assert!(buffer.is_reading_complete());

                let mut oracle_results = results.pop().unwrap();
                let actual_results = results.pop().unwrap();

                assert_eq!(
                    oracle_results.1, actual_results.1,
                    "the lengths should match"
                );

                // make sure the buffers match
                crate::slice::zip(&actual_results.0, &mut oracle_results.0, |a, b| {
                    assert_eq!(a, b, "all chunk bytes should match");
                });

                assert!(
                    oracle_results.2 >= actual_results.2,
                    "the actual allocations should be no worse than the oracle"
                );

                if reverse {
                    let ideal_allocation = (partial_size + fin_size) as u64;
                    assert_eq!(
                        actual_results.2, ideal_allocation,
                        "if the chunks were reversed, the allocation should be ideal"
                    );
                }
            }
        }
    }
}

#[test]
fn write_fin_zero_test() {
    let mut buffer = Reassembler::new();

    buffer.write_at_fin(0u32.into(), &[]).unwrap();

    assert_eq!(
        buffer.write_at(0u32.into(), &[1]),
        Err(Error::InvalidFin),
        "no data can be written after a fin"
    );
}

#[test]
fn fin_pop_take_test() {
    for write_fin in [false, true] {
        let mut buffer = Reassembler::new();
        buffer.write_at(0u32.into(), &[1]).unwrap();
        if write_fin {
            buffer.write_at_fin(1u32.into(), &[]).unwrap();
        }

        let chunk = buffer.pop().unwrap();
        assert_eq!(chunk.len(), 1);

        // if we wrote a fin, then we should get the entire chunk so the BytesMut doesn't get
        // promoted to shared mode
        //
        // See https://github.com/tokio-rs/bytes/blob/64c4fa286771ad9e522ffbefc576bcf7b76933d0/src/bytes_mut.rs#L975
        if write_fin {
            assert_eq!(chunk.capacity(), 4096);
        } else {
            assert_eq!(chunk.capacity(), 1);
        }
    }
}

#[test]
fn write_fin_changed_error_test() {
    let mut buffer = Reassembler::new();

    buffer.write_at_fin(16u32.into(), &[]).unwrap();

    assert_eq!(
        buffer.write_at_fin(0u32.into(), &[]),
        Err(Error::InvalidFin),
        "the fin cannot decrease a previous fin"
    );
    assert_eq!(
        buffer.write_at_fin(32u32.into(), &[]),
        Err(Error::InvalidFin),
        "the fin cannot exceed a previous fin"
    );
}

#[test]
fn write_fin_lowered_test() {
    let mut buffer = Reassembler::new();

    buffer.write_at(32u32.into(), &[1]).unwrap();

    assert_eq!(
        buffer.write_at_fin(16u32.into(), &[]),
        Err(Error::InvalidFin),
        "the fin cannot be lower than an already existing chunk"
    );
}

#[test]
fn write_fin_complete_test() {
    let mut buffer = Reassembler::new();

    buffer.write_at_fin(4u32.into(), &[4]).unwrap();

    assert!(!buffer.is_writing_complete());
    assert!(!buffer.is_reading_complete());

    buffer.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();

    assert!(buffer.is_writing_complete());
    assert!(!buffer.is_reading_complete());

    buffer.pop().unwrap();

    assert!(buffer.is_writing_complete());
    assert!(buffer.is_reading_complete());
}

#[test]
fn allocation_size_test() {
    // | bytes received | allocation size |
    // |----------------|-----------------|
    // | 0              | 4096            |
    // | 65536          | 16384           |
    // | 262144         | 32768           |
    // | 1048576        | 65536           |

    let received = [(0, 4096), (65536, 16384), (262144, 32768), (1048576, 65536)];

    for (index, (offset, size)) in received.iter().copied().enumerate() {
        assert_eq!(
            Reassembler::allocation_size(offset),
            size,
            "offset = {offset}"
        );

        if let Some((offset, _)) = received.get(index + 1) {
            let offset = offset - 1;
            assert_eq!(
                Reassembler::allocation_size(offset),
                size,
                "offset = {offset}"
            );
        }
    }
}
