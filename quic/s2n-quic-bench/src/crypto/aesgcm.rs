// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::convert::TryInto;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_quic_crypto::testing::{
    self,
    aesgcm::{Key, NONCE_LEN, TAG_LEN},
};

struct Implementation {
    key: Key,
    name: &'static str,
}

macro_rules! impls {
    ($name:ident) => {{
        use s2n_quic_crypto::testing::aes::$name::KEY_LEN;

        let mut impls = vec![];

        for imp in testing::aesgcm::$name::implementations() {
            impls.push(Implementation {
                key: imp.new(black_box([1; KEY_LEN])),
                name: imp.name(),
            });
        }

        (stringify!($name), impls)
    }};
}

pub fn benchmarks(c: &mut Criterion) {
    let impls = [impls!(aes128), impls!(aes256)];

    // create some shared values
    let nonce = black_box([123u8; NONCE_LEN]);
    let aad = black_box([123u8; 20]);

    for (group, impls) in impls.iter() {
        let mut encrypt = c.benchmark_group(format!("crypto/aesgcm/{group}/encrypt"));
        for imp in impls.iter() {
            for block in testing::BLOCK_SIZES.iter() {
                encrypt.throughput(Throughput::Bytes(block.len() as _));
                encrypt.bench_with_input(
                    BenchmarkId::new(imp.name, block),
                    &block,
                    move |b, block| {
                        let key = &imp.key;

                        let mut input = block.to_vec();
                        let payload_len = input.len();
                        input.extend_from_slice(&[0; TAG_LEN]);

                        let (payload, tag) = input.split_at_mut(payload_len);
                        let tag: &mut [u8; TAG_LEN] = tag.try_into().unwrap();
                        b.iter(|| {
                            let _ = key.encrypt(&nonce, &aad, payload, tag);
                        });
                    },
                );
            }
        }
        encrypt.finish();

        let mut decrypt = c.benchmark_group(format!("crypto/aesgcm/{group}/decrypt"));
        for imp in impls.iter() {
            for block in testing::BLOCK_SIZES.iter() {
                decrypt.throughput(Throughput::Bytes(block.len() as _));
                decrypt.bench_with_input(BenchmarkId::new(imp.name, block), &block, |b, block| {
                    let key = &imp.key;

                    let mut input = block.to_vec();
                    let payload_len = input.len();
                    input.extend_from_slice(&[0; TAG_LEN]);

                    let (payload, tag) = input.split_at_mut(payload_len);
                    let tag: &mut [u8; TAG_LEN] = tag.try_into().unwrap();

                    // create a valid encrypted payload
                    key.encrypt(&nonce, &aad, payload, tag).unwrap();
                    let tag = &*tag;

                    b.iter_batched(
                        || payload.to_vec(),
                        |mut payload| {
                            let _ = black_box(key.decrypt(&nonce, &aad, &mut payload, tag));
                        },
                        criterion::BatchSize::LargeInput,
                    );
                });
            }
        }
        decrypt.finish();
    }
}
