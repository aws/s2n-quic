// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

pub fn benchmarks(c: &mut Criterion) {
    psk(c);
}

fn psk(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto/hkdf/psk");

    group.throughput(Throughput::Elements(1));

    let prk_lens = [16, 32];
    let key_lens = [16, 32, 64];
    let label_lens = [1, 8, 16, 32, 64];
    let algs = [
        ("sha256", awslc::HKDF_SHA256),
        ("sha384", awslc::HKDF_SHA384),
        ("sha512", awslc::HKDF_SHA512),
    ];

    for prk_len in prk_lens {
        for key_len in key_lens {
            for label_len in label_lens {
                for (alg_name, alg) in algs {
                    group.bench_with_input(
                        BenchmarkId::new(
                            format!("{alg_name}_reuse"),
                            format!("prk_len={prk_len},label_len={label_len},out_len={key_len}"),
                        ),
                        &key_len,
                        |b, &key_len| {
                            let prk = black_box(awslc::prk(&vec![42u8; prk_len], alg));
                            let label = black_box(vec![42u8; label_len]);
                            let mut out = black_box(vec![0u8; key_len]);
                            b.iter(move || {
                                awslc::derive_psk(&prk, &label, &mut out);
                            });
                        },
                    );
                    group.bench_with_input(
                        BenchmarkId::new(
                            format!("{alg_name}_fresh"),
                            format!("prk_len={prk_len},label_len={label_len},out_len={key_len}"),
                        ),
                        &key_len,
                        |b, &key_len| {
                            let key = black_box(vec![42u8; prk_len]);
                            let label = black_box(vec![42u8; label_len]);
                            let mut out = black_box(vec![0u8; key_len]);
                            b.iter(move || {
                                let prk = black_box(awslc::prk(&key, alg));
                                awslc::derive_psk(&prk, &label, &mut out);
                            });
                        },
                    );
                }
            }
        }
    }
}

mod awslc {
    pub use aws_lc_rs::hkdf::*;

    #[inline(never)]
    pub fn prk(prk: &[u8], alg: Algorithm) -> Prk {
        Prk::new_less_safe(alg, prk)
    }

    #[inline(never)]
    pub fn derive_psk(prk: &Prk, label: &[u8], out: &mut [u8]) {
        let out_len = out.len();
        let out_len = OutLen(out_len);

        prk.expand(&[label], out_len)
            .unwrap()
            .fill(&mut out[..out_len.0])
            .unwrap();
    }

    #[derive(Clone, Copy)]
    struct OutLen(usize);

    impl KeyType for OutLen {
        fn len(&self) -> usize {
            self.0
        }
    }
}
