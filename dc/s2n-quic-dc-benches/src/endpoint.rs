// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::bench::{black_box, BatchSize, BenchmarkId, Criterion, Throughput};

pub fn benchmarks(c: &mut Criterion) {
    assemble_benches(c);
    ack_processing_benches(c);
}

fn assemble_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("endpoint/assemble");
    let scenarios = [
        (16usize, 1usize, 32usize),
        (16, 8, 32),
        (64, 8, 32),
        (64, 16, 16),
        (128, 16, 16),
    ];

    for (packets, frames_per_packet, payload_len) in scenarios {
        group.throughput(Throughput::Elements((packets * frames_per_packet) as u64));
        let input_name =
            format!("packets={packets},frames={frames_per_packet},payload={payload_len}");
        group.bench_with_input(BenchmarkId::new("assemble", &input_name), &(), |b, _| {
            b.iter_batched(
                || {
                    s2n_quic_dc::endpoint::testing::bench::AssembleBenchmark::new(
                        packets,
                        frames_per_packet,
                        payload_len,
                    )
                },
                |benchmark| {
                    black_box(benchmark.run());
                },
                BatchSize::SmallInput,
            );
        });
    }
}

fn ack_processing_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("endpoint/ack_processing");
    let scenarios = [
        (16usize, 1usize, 32usize, 1usize),
        (64, 1, 32, 1),
        (64, 8, 32, 1),
        (128, 8, 16, 4),
        (256, 4, 16, 8),
    ];

    for (packets, frames_per_packet, payload_len, ack_frames) in scenarios {
        group.throughput(Throughput::Elements(packets as u64));
        let input_name = format!(
            "packets={packets},frames={frames_per_packet},payload={payload_len},ack_frames={ack_frames}"
        );
        group.bench_with_input(BenchmarkId::new("ack", &input_name), &(), |b, _| {
            b.iter_batched(
                || {
                    s2n_quic_dc::endpoint::testing::bench::AckProcessingBenchmark::new(
                        packets,
                        frames_per_packet,
                        payload_len,
                        ack_frames,
                    )
                },
                |benchmark| {
                    black_box(benchmark.run());
                },
                BatchSize::SmallInput,
            );
        });
    }
}
