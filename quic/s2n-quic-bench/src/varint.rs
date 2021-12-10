// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion};
use s2n_codec::{DecoderBuffer, EncoderBuffer, EncoderValue};
use s2n_quic_core::varint::VarInt;

pub fn benchmarks(c: &mut Criterion) {
    round_trip(c);
}

fn round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint");
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61] {
        let i = VarInt::new(2u64.pow(i)).unwrap();

        group.bench_with_input(BenchmarkId::new("round_trip", i), &i, |b, input| {
            let mut buffer = vec![0; 8];
            b.iter(|| {
                input.encode(&mut EncoderBuffer::new(&mut buffer));
                let (actual, _) = DecoderBuffer::new(&buffer).decode::<VarInt>().unwrap();
                black_box(actual);
            });
        });
    }
    group.finish();
}
