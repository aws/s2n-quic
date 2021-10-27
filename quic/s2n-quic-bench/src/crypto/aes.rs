// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use s2n_quic_crypto::testing;

macro_rules! aes_impl {
    ($name:ident) => {
        fn $name(c: &mut Criterion) {
            aes_impl!(c, $name, encrypt);
            aes_impl!(c, $name, decrypt);
        }
    };
    ($c:ident, $name:ident, $call:ident) => {{
        let mut group = $c.benchmark_group(concat!(
            "crypto/aes/",
            stringify!($name),
            "/",
            stringify!($call)
        ));
        for imp in testing::aes::$name::implementations() {
            for block in testing::BLOCK_SIZES.iter() {
                group.throughput(Throughput::Bytes(block.len() as _));
                group.bench_with_input(BenchmarkId::new(imp.name(), block), &block, |b, block| {
                    let key = imp.new([1; testing::aes::$name::KEY_LEN]);
                    let mut input = block.to_vec();
                    b.iter(|| key.$call(&mut input));
                });
            }
        }
        group.finish();
    }};
}

aes_impl!(aes128);
aes_impl!(aes256);

pub fn benchmarks(c: &mut Criterion) {
    aes128(c);
    aes256(c);
}
