// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use s2n_quic_core::{buffer::ReceiveBuffer, varint::VarInt};

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer");

    for size in [1, 100, 1000, 1450] {
        let input = vec![42u8; size];

        group.throughput(Throughput::Bytes(input.len() as _));
        group.bench_with_input(BenchmarkId::new("write_at", size), &input, |b, input| {
            let mut buffer = ReceiveBuffer::new();
            let mut offset = VarInt::from_u8(0);
            let len = VarInt::new(input.len() as _).unwrap();
            b.iter(move || {
                buffer.write_at(offset, input).unwrap();
                offset += len;
            });
        });
    }
}
