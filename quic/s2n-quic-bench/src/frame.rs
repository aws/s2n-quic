// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::frame::FrameMut;
use std::hint::black_box;

pub fn benchmarks(c: &mut Criterion) {
    codec(c);
}

struct Input {
    name: &'static str,
    buffer: &'static [u8],
}

macro_rules! input {
    ($name:expr) => {
        Input {
            name: $name,
            buffer: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../s2n-quic-core/src/frame/test_samples/",
                $name,
                ".bin"
            )),
        }
    };
}

fn codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");

    let mut inputs = [
        input!("ack"),
        input!("connection_close"),
        input!("crypto"),
        input!("data_blocked"),
        input!("handshake_done"),
        input!("max_data"),
        input!("max_stream_data"),
        input!("max_streams"),
        input!("new_connection_id"),
        input!("new_token"),
        input!("padding"),
        input!("path_challenge"),
        input!("path_response"),
        input!("ping"),
        input!("reset_stream"),
        input!("retire_connection_id"),
        input!("stop_sending"),
        input!("stream"),
        input!("stream_data_blocked"),
        input!("streams_blocked"),
    ];

    // sort by length to make the graphs nicer
    inputs.sort_by_key(|input| input.buffer.len());

    for input in &inputs {
        group.throughput(Throughput::Bytes(input.buffer.len() as _));
        group.bench_with_input(
            BenchmarkId::new("decode", input.name),
            &input.buffer,
            |b, buffer| {
                let mut buffer = buffer.to_vec();
                b.iter(move || {
                    let buffer = DecoderBufferMut::new(&mut buffer);
                    let _ = black_box(buffer.decode::<FrameMut>());
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("encode", input.name),
            &input.buffer,
            |b, buffer| {
                let mut buffer = buffer.to_vec();
                let buffer = DecoderBufferMut::new(&mut buffer);
                let (frame, _remaining) = buffer.decode::<FrameMut>().unwrap();
                let frame = black_box(frame);
                let mut buffer = vec![0; frame.encoding_size()];
                b.iter(move || {
                    EncoderBuffer::new(&mut buffer).encode(&frame);
                });
            },
        );
    }

    group.finish();
}
