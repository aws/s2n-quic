// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, Criterion, Throughput};

pub fn benchmarks(c: &mut Criterion) {
    init(c);
}

fn init(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto/hmac/init");

    group.throughput(Throughput::Elements(1));

    let algs = [
        ("sha256", aws_lc_rs::hmac::HMAC_SHA256),
        ("sha384", aws_lc_rs::hmac::HMAC_SHA384),
        ("sha512", aws_lc_rs::hmac::HMAC_SHA512),
    ];

    for (alg_name, alg) in algs {
        group.bench_function(format!("{alg_name}_init"), |b| {
            let key_len = aws_lc_rs::hkdf::KeyType::len(&alg);
            let mut key = vec![0u8; key_len];
            aws_lc_rs::rand::fill(&mut key).unwrap();
            let key = black_box(&key);
            b.iter(move || {
                let _ = black_box(aws_lc_rs::hmac::Key::new(alg, key));
            });
        });
    }
}
