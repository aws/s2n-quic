// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use crossbeam_channel::bounded;
use s2n_quic_core::sync::spsc;

pub fn benchmarks(c: &mut Criterion) {
    spsc_benches(c);
}

fn spsc_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("spsc");

    for i in [1, 64, 1024, 4096] {
        group.throughput(Throughput::Elements(i as _));
        group.bench_with_input(BenchmarkId::new("s2n/send_recv", i), &i, |b, input| {
            let (mut sender, mut receiver) = spsc::channel(*input);
            b.iter(|| {
                {
                    let mut slice = sender.try_slice().unwrap().unwrap();
                    while slice.push(123usize).is_ok() {}
                }

                {
                    let mut slice = receiver.try_slice().unwrap().unwrap();
                    while slice.pop().is_some() {}
                }
            });
        });
        group.bench_with_input(
            BenchmarkId::new("crossbeam/send_recv", i),
            &i,
            |b, input| {
                let (sender, receiver) = bounded(*input);
                b.iter(|| {
                    {
                        while sender.try_send(123usize).is_ok() {}
                    }

                    {
                        while receiver.try_recv().is_ok() {}
                    }
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("s2n/send_recv_iter", i), &i, |b, input| {
            let (mut sender, mut receiver) = spsc::channel(*input);
            b.iter(|| {
                {
                    let mut slice = sender.try_slice().unwrap().unwrap();
                    let _ = slice.extend(&mut core::iter::repeat(123usize));
                }

                {
                    let mut slice = receiver.try_slice().unwrap().unwrap();
                    slice.clear();
                }
            });
        });
        group.bench_with_input(
            BenchmarkId::new("crossbeam/send_recv_iter", i),
            &i,
            |b, input| {
                let (sender, receiver) = bounded(*input);
                b.iter(|| {
                    {
                        while sender.try_send(123usize).is_ok() {}
                    }

                    {
                        for _ in receiver.try_iter() {}
                    }
                });
            },
        );
    }
    group.finish();
}
