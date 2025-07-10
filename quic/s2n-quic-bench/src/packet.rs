// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{connection::id::ConnectionInfo, inet::SocketAddress, packet::ProtectedPacket};
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
                "/../s2n-quic-core/src/packet/test_samples/",
                $name,
                ".bin"
            )),
        }
    };
}

fn codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet");

    let mut inputs = [
        input!("handshake"),
        input!("initial"),
        input!("short"),
        input!("zero_rtt"),
        input!("version_negotiation"),
        input!("retry"),
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
                    let remote_address = SocketAddress::default();
                    let connection_info = ConnectionInfo::new(&remote_address);
                    let _ = black_box(ProtectedPacket::decode(buffer, &connection_info, &20));
                })
            },
        );
    }

    group.finish();
}
