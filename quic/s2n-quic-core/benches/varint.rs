// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use s2n_codec::{DecoderBuffer, EncoderBuffer, EncoderValue};
use s2n_quic_core::varint::VarInt;

fn round_trip(c: &mut Criterion) {
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61].iter() {
        c.bench_function(&format!("round trip 2^{}", i), move |b| {
            let expected = black_box(VarInt::new(2u64.pow(*i)).unwrap());
            let mut buffer = vec![0; 8];

            b.iter(move || {
                expected.encode(&mut EncoderBuffer::new(&mut buffer));
                let (actual, _) = DecoderBuffer::new(&buffer).decode::<VarInt>().unwrap();
                assert_eq!(actual, expected);
            })
        });
    }
}

criterion_group!(benches, round_trip);
criterion_main!(benches);
