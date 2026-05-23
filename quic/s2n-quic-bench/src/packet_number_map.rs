// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion, Throughput};
use s2n_quic_core::packet::number::{
    map::SortedVecMap, Map, PacketNumberRange, PacketNumberSpace,
};
use s2n_quic_core::varint::VarInt;
use std::hint::black_box;

pub fn benchmarks(c: &mut Criterion) {
    remove_range_sparse(c);
    remove_range_front(c);
    remove_range_dense(c);
    insert_sequential(c);
}

/// Models the pathological case from the ack-processor-scalability doc:
/// ~N entries spread across a much larger PN span, with ACK ranges covering
/// mostly-empty regions.
fn remove_range_sparse(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_number_map/remove_range_sparse");

    // (pending_entries, span_size, ack_range_size)
    let cases: &[(usize, u64, u64)] = &[
        (50, 500, 200),
        (100, 2000, 1000),
        (439, 5000, 2000),  // production p50
        (439, 11000, 5000),
        (953, 11000, 5000), // production p99
    ];

    for &(pending, span, ack_range) in cases {
        let label = format!("pending={pending}_span={span}_ack={ack_range}");

        group.throughput(Throughput::Elements(1));

        // Ring buffer (baseline)
        group.bench_with_input(
            BenchmarkId::new("ring_buffer", &label),
            &(pending, span, ack_range),
            |b, &(pending, span, ack_range)| {
                b.iter_batched(
                    || build_sparse_ring(pending, span),
                    |mut map| {
                        let range = sparse_ack_range(span, ack_range);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        // SortedVecMap (candidate)
        group.bench_with_input(
            BenchmarkId::new("sorted_vec", &label),
            &(pending, span, ack_range),
            |b, &(pending, span, ack_range)| {
                b.iter_batched(
                    || build_sparse_sorted(pending, span),
                    |mut map| {
                        let range = sparse_ack_range(span, ack_range);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Front removal: simulates loss detection (remove PNs 0..max_acked-3).
/// This is the most common pattern and where VecDeque excels.
fn remove_range_front(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_number_map/remove_range_front");

    // (total_entries, entries_to_remove_from_front)
    let cases: &[(usize, usize)] = &[(100, 30), (439, 100), (953, 200)];

    for &(total, remove_count) in cases {
        let label = format!("total={total}_remove={remove_count}");
        group.throughput(Throughput::Elements(remove_count as u64));

        group.bench_with_input(
            BenchmarkId::new("ring_buffer", &label),
            &(total, remove_count),
            |b, &(total, remove_count)| {
                b.iter_batched(
                    || build_dense_ring(total),
                    |mut map| {
                        let start =
                            PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(0));
                        let end = PacketNumberSpace::Initial
                            .new_packet_number(VarInt::new(remove_count as u64 - 1).unwrap());
                        let range = PacketNumberRange::new(start, end);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sorted_vec", &label),
            &(total, remove_count),
            |b, &(total, remove_count)| {
                b.iter_batched(
                    || build_dense_sorted(total),
                    |mut map| {
                        let start =
                            PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(0));
                        let end = PacketNumberSpace::Initial
                            .new_packet_number(VarInt::new(remove_count as u64 - 1).unwrap());
                        let range = PacketNumberRange::new(start, end);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Dense case: all entries are contiguous (best case for ring buffer).
fn remove_range_dense(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_number_map/remove_range_dense");

    let cases: &[(usize, u64)] = &[(100, 50), (439, 200), (1000, 500)];

    for &(total, ack_range) in cases {
        let label = format!("total={total}_ack={ack_range}");
        group.throughput(Throughput::Elements(ack_range));

        // Ring buffer
        group.bench_with_input(
            BenchmarkId::new("ring_buffer", &label),
            &(total, ack_range),
            |b, &(total, ack_range)| {
                b.iter_batched(
                    || build_dense_ring(total),
                    |mut map| {
                        let range = dense_ack_range(ack_range);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        // SortedVecMap
        group.bench_with_input(
            BenchmarkId::new("sorted_vec", &label),
            &(total, ack_range),
            |b, &(total, ack_range)| {
                b.iter_batched(
                    || build_dense_sorted(total),
                    |mut map| {
                        let range = dense_ack_range(ack_range);
                        let count: usize = black_box(map.remove_range(range).count());
                        black_box(count);
                        black_box(map);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn insert_sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_number_map/insert");

    for &count in &[100u64, 500, 1000] {
        group.throughput(Throughput::Elements(count));

        group.bench_with_input(
            BenchmarkId::new("ring_buffer", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut map: Map<u64> = Map::default();
                    for i in 0..count {
                        let pn =
                            PacketNumberSpace::Initial.new_packet_number(VarInt::new(i).unwrap());
                        map.insert(pn, i);
                    }
                    black_box(map);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sorted_vec", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut map: SortedVecMap<u64> = SortedVecMap::default();
                    for i in 0..count {
                        let pn =
                            PacketNumberSpace::Initial.new_packet_number(VarInt::new(i).unwrap());
                        map.insert(pn, i);
                    }
                    black_box(map);
                });
            },
        );
    }

    group.finish();
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn sparse_ack_range(span: u64, ack_range: u64) -> PacketNumberRange {
    let ack_start = span / 4;
    let ack_end = (ack_start + ack_range).min(span);
    let start = PacketNumberSpace::Initial.new_packet_number(VarInt::new(ack_start).unwrap());
    let end = PacketNumberSpace::Initial.new_packet_number(VarInt::new(ack_end).unwrap());
    PacketNumberRange::new(start, end)
}

fn dense_ack_range(ack_range: u64) -> PacketNumberRange {
    let start = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(0));
    let end =
        PacketNumberSpace::Initial.new_packet_number(VarInt::new(ack_range - 1).unwrap());
    PacketNumberRange::new(start, end)
}

fn build_sparse_ring(pending: usize, span: u64) -> Map<u64> {
    let mut map = Map::default();
    let step = span / pending as u64;
    for i in 0..pending {
        let pn_val = (i as u64) * step;
        let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::new(pn_val).unwrap());
        map.insert(pn, pn_val);
    }
    map
}

fn build_sparse_sorted(pending: usize, span: u64) -> SortedVecMap<u64> {
    let mut map = SortedVecMap::default();
    let step = span / pending as u64;
    for i in 0..pending {
        let pn_val = (i as u64) * step;
        let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::new(pn_val).unwrap());
        map.insert(pn, pn_val);
    }
    map
}

fn build_dense_ring(total: usize) -> Map<u64> {
    let mut map = Map::default();
    for i in 0..total as u64 {
        let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::new(i).unwrap());
        map.insert(pn, i);
    }
    map
}

fn build_dense_sorted(total: usize) -> SortedVecMap<u64> {
    let mut map = SortedVecMap::default();
    for i in 0..total as u64 {
        let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::new(i).unwrap());
        map.insert(pn, i);
    }
    map
}
