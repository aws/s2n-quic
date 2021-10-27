// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_quic_crypto::testing;

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto/ghash");
    for imp in testing::ghash::implementations() {
        for block in testing::BLOCK_SIZES.iter() {
            group.throughput(Throughput::Bytes(block.len() as _));
            group.bench_with_input(BenchmarkId::new(imp.name(), block), &block, |b, block| {
                let key = imp.new([1; 16]);
                let input = &block[..];
                b.iter(|| black_box(key.hash(input)))
            });
        }
    }
    group.finish();
}
