use crate::buffer::{
    StreamReceiveBuffer, StreamReceiveBufferError, DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE,
    MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE,
};
use core::ops::Deref;
use s2n_quic_core::varint::{VarInt, MAX_VARINT_VALUE};

fn new_receive_buffer() -> StreamReceiveBuffer {
    let buffer = StreamReceiveBuffer::new();
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
    assert_eq!(4, buffer.len());

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
    let mut buf = StreamReceiveBuffer::new();

    assert_eq!(0, buf.len());
    assert_eq!(true, buf.is_empty());
    assert_eq!(None, buf.pop());

    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(4, buf.len());
    assert_eq!(false, buf.is_empty());
    assert_eq!(&[0u8, 1, 2, 3], buf.pop().unwrap().deref());
    assert_eq!(0, buf.len());
    assert_eq!(true, buf.is_empty());
    assert_eq!(None, buf.pop());

    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    buf.write_at(2u32.into(), &[2, 3, 4, 5, 6]).unwrap();
    assert_eq!(3, buf.len());
    assert_eq!(&[4, 5, 6], buf.pop().unwrap().deref());

    // Create a gap
    buf.write_at(10u32.into(), &[10, 11, 12]).unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    // Fill the gap
    buf.write_at(7u32.into(), &[7, 8, 9]).unwrap();
    assert_eq!(6, buf.len());
    assert_eq!(&[7, 8, 9, 10, 11, 12], buf.pop().unwrap().deref());
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
    assert_eq!(
        &[13, 14, 15, 16, 17, 18, 19, 20],
        buf.pop().unwrap().deref()
    );
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());
}

#[test]
fn fill_preallocated_gaps() {
    let mut buf: StreamReceiveBuffer = StreamReceiveBuffer::new();

    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32 + 2).into(),
        &[42, 45],
    )
    .unwrap();
    assert_eq!(0, buf.len());
    assert_eq!(None, buf.pop());

    let mut first_chunk = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 2];
    for (idx, b) in first_chunk.iter_mut().enumerate() {
        *b = idx as u8;
    }

    buf.write_at(0u32.into(), &first_chunk).unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 2, buf.len());

    // Overfill gap
    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32 - 4).into(),
        &[91, 92, 93, 94, 95, 96, 97, 98, 99],
    )
    .unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE + 5, buf.len());

    let chunk = buf.pop().unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, chunk.len());
    let mut expected = 0;
    for b in &chunk[..DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 2] {
        assert_eq!(expected, *b);
        expected = expected.wrapping_add(1);
    }
    assert_eq!(93, chunk[DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 2]);
    assert_eq!(94, chunk[DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 1]);

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
    let mut buf = StreamReceiveBuffer::new();
    // This creates 3 full buffer gaps of full allocation ranges
    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32 * 3 + 2).into(),
        &[7, 8, 9],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Insert something in the middle, which still leaves us with 2 gaps
    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32 + 1).into(),
        &[10, 11, 12],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Close the last gap
    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32 * 2 + 1).into(),
        &[14, 15, 16],
    )
    .unwrap();
    assert_eq!(0, buf.len());

    // Close the first gap
    buf.write_at(4u32.into(), &[4, 5, 6, 7]).unwrap();
    assert_eq!(0, buf.len());

    // Now fill all the preallocated chunks with data
    let data = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE * 3 + 20];
    buf.write_at(0u32.into(), &data).unwrap();
    assert_eq!(
        DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE * 3 + 20,
        buf.len()
    );

    fn assert_zero_content(slice: &[u8]) {
        for b in slice {
            assert_eq!(0, *b);
        }
    }

    // Check the first allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..4]);
    assert_eq!(&[4, 5, 6, 7], &chunk[4..8]);
    assert_zero_content(&chunk[8..DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE]);

    // Check the second allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..1]);
    assert_eq!(&[10, 11, 12], &chunk[1..4]);
    assert_zero_content(&chunk[4..DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE]);

    // Check the third allocation range
    let chunk = buf.pop().unwrap();
    assert_eq!(DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, chunk.len());
    assert_zero_content(&chunk[..1]);
    assert_eq!(&[14, 15, 16], &chunk[1..4]);
    assert_zero_content(&chunk[4..DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE]);

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
    let mut buf = StreamReceiveBuffer::new();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(0, buf.consumed_len());
    assert_eq!(&[0u8, 1, 2, 3], buf.pop().unwrap().deref());
    assert_eq!(4, buf.consumed_len());

    // Add partially consumed data
    buf.write_at(2u32.into(), &[2, 3, 4]).unwrap();
    assert_eq!(4, buf.consumed_len());
    assert_eq!(&[4], buf.pop().unwrap().deref());
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
    let mut buf = StreamReceiveBuffer::new();
    buf.write_at(4u32.into(), &[4, 5, 6]).unwrap();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(7, buf.len());
    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6], buf.pop().unwrap().deref());
}

#[test]
fn merge_left() {
    let mut buf = StreamReceiveBuffer::new();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    buf.write_at(4u32.into(), &[4, 5, 6]).unwrap();
    assert_eq!(7, buf.len());
    assert_eq!(&[0u8, 1, 2, 3, 4, 5, 6], buf.pop().unwrap().deref());
}

#[test]
fn merge_both_sides() {
    let mut buf = StreamReceiveBuffer::new();
    // Create gaps on all sides, and merge them later
    buf.write_at(4u32.into(), &[4, 5]).unwrap();
    buf.write_at(8u32.into(), &[8, 9]).unwrap();
    buf.write_at(6u32.into(), &[6, 7]).unwrap();
    buf.write_at(0u32.into(), &[0, 1, 2, 3]).unwrap();
    assert_eq!(10, buf.len());
    assert_eq!(
        &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        buf.pop().unwrap().deref()
    );
}

#[test]
fn do_not_merge_across_allocations_right() {
    let mut buf = StreamReceiveBuffer::new();
    let mut data_left = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE];
    let mut data_right = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE];

    data_left[DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 1] = 13;
    data_right[0] = 72;

    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32).into(),
        &data_right[..],
    )
    .unwrap();
    buf.write_at(0u32.into(), &data_left[..]).unwrap();
    assert_eq!(2 * DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, buf.len());

    assert_eq!(&data_left[..], buf.pop().unwrap().deref());
    assert_eq!(&data_right[..], buf.pop().unwrap().deref());
}

#[test]
fn do_not_merge_across_allocations_left() {
    let mut buf = StreamReceiveBuffer::new();
    let mut data_left = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE];
    let mut data_right = [0u8; DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE];

    data_left[DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE - 1] = 13;
    data_right[0] = 72;

    buf.write_at(0u32.into(), &data_left[..]).unwrap();
    buf.write_at(
        (DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE as u32).into(),
        &data_right[..],
    )
    .unwrap();
    assert_eq!(2 * DEFAULT_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE, buf.len());

    assert_eq!(&data_left[..], buf.pop().unwrap().deref());
    assert_eq!(&data_right[..], buf.pop().unwrap().deref());
}

#[test]
fn use_custom_buffer_size() {
    // This must be at least 32 bytes - otherwise the buffers will use
    // `BytesMut` short-byte optimization, which at least allocates
    // 4 * sizeof::<usize> - 1 == 31 bytes.
    // bytes.
    let mut buf = StreamReceiveBuffer::with_buffer_size(MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE);

    let mut data = [0u8; 3 * MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE + 3];
    for (i, item) in data.iter_mut().enumerate() {
        *item = i as u8;
    }

    buf.write_at(0u32.into(), &data[..]).unwrap();
    for i in 0..3 {
        assert_eq!(
            &data[i * MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE
                ..(i + 1) * MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE],
            buf.pop().unwrap().deref()
        );
    }

    assert_eq!(
        &data[3 * MIN_STREAM_RECEIVE_BUFFER_ALLOCATION_SIZE..],
        buf.pop().unwrap().deref()
    );
}

#[test]
#[should_panic]
fn fails_with_buffer_size_below_minimum() {
    let _buffer = StreamReceiveBuffer::with_buffer_size(1);
}

#[test]
fn reset_buffer() {
    let mut buf = StreamReceiveBuffer::new();
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
    let mut buffer = StreamReceiveBuffer::with_buffer_size(64);

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
    let mut buffer = StreamReceiveBuffer::with_buffer_size(64);

    for nr_bytes in 0..64 * 2 + 1 {
        let data = vec![0u8; nr_bytes + 1];
        assert_eq!(
            Err(StreamReceiveBufferError::OutOfRange),
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
    let mut buffer = StreamReceiveBuffer::new();

    // Overflow the allowed buffers by size 1. This uses an invalid memory
    // reference, due to not wanting to allocate too much memory. This is
    // ok in order to make sure that the overflow check works.
    // fake_data is based on a valid base pointer, in order to give us a
    // bit more certainity that the test won't fail for other reasons.
    let data = [0u8; 32];
    let fake_data = unsafe {
        core::slice::from_raw_parts(&data as *const u8, MAX_VARINT_VALUE as usize - 20 + 1)
    };

    for _ in 0..64 * 2 + 1 {
        assert_eq!(
            Err(StreamReceiveBufferError::OutOfRange),
            buffer.write_at(20u32.into(), fake_data)
        );
    }
}

#[test]
fn pop_watermarked_test() {
    let mut buffer = StreamReceiveBuffer::new();

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
