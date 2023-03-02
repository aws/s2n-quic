// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::hash::Hasher;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_quic_core::inet::checksum::Checksum;

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("inet");
    for block in [1500, 9000, 1 << 16] {
        let data = vec![123u8; block];
        group.throughput(Throughput::Bytes(block as u64));
        group.bench_with_input(
            BenchmarkId::new("s2n/checksum", block),
            &data,
            |b, block| {
                let cs = Checksum::default();
                let input = black_box(&block[..]);
                b.iter(|| {
                    let mut checksum = cs;
                    checksum.write(input);
                    black_box(checksum.finish())
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("fuchsia/checksum", block),
            &data,
            |b, block| {
                let input = black_box(&block[..]);
                b.iter(|| black_box(internet_checksum::checksum(input)))
            },
        );
    }
    group.finish();
}
