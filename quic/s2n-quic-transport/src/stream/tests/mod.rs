// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    frame::stream::Stream as StreamFrame, stream::StreamId, transport, varint::VarInt,
};

pub use crate::contexts::testing::*;
mod test_environment;
pub use test_environment::*;

macro_rules! assert_matches {
    ($a:expr, $b:pat $(,)?) => {
        match $a {
            $b => {}
            ref value => {
                panic!("value {:?} did not match {}", value, stringify!($b))
            }
        }
    };
}
pub(super) use assert_matches;

/// Creates a `STREAM_DATA` frame
pub fn stream_data<Data>(
    stream_id: StreamId,
    offset: VarInt,
    data: Data,
    is_fin: bool,
) -> StreamFrame<Data> {
    StreamFrame {
        offset,
        data,
        stream_id: stream_id.into(),
        is_last_frame: false,
        is_fin,
    }
}

/// Asserts that a `Result` type contains a TransportError with the given
/// error code.
pub fn assert_is_transport_error<T: core::fmt::Debug>(
    result: Result<T, transport::Error>,
    expected: transport::Error,
) {
    let actual = result.unwrap_err();
    assert_eq!(expected.code, actual.code);
}

/// Generates test data using a pattern which is identifieable. For a given
/// offset in the Stream the utilized data will always be the same. This allows
/// us to do some simple validation checking whether a receiver received the
/// expected data without exactly knowing the actual sent data.
pub fn gen_pattern_test_data(offset: VarInt, len: usize) -> Vec<u8> {
    let mut data = Vec::new();
    data.reserve(len);

    fn data_for_offset(offset: u64) -> u8 {
        (offset % 256) as u8
    }

    for i in 0..len {
        let current_offset: u64 = Into::<u64>::into(offset) + i as u64;
        data.push(data_for_offset(current_offset));
    }

    data
}

pub fn gen_pattern_test_chunks(mut offset: VarInt, lens: &[usize]) -> Vec<bytes::Bytes> {
    lens.iter()
        .map(|size| {
            let data = bytes::Bytes::from(gen_pattern_test_data(offset, *size));
            offset += *size;
            data
        })
        .collect::<Vec<_>>()
}

#[test]
fn idle_stream_does_not_write_data() {
    let mut test_env = setup_stream_test_env();
    test_env.assert_write_frames(0);
}
