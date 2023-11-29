// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_quic_core::{buffer::ReceiveBuffer, varint::VarInt};

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer");

    for size in [1, 100, 1000, 1450, 8500, 9000] {
        let input = vec![42u8; size];

        group.throughput(Throughput::Bytes(input.len() as _));

        group.bench_with_input(BenchmarkId::new("skip", size), &input, |b, _input| {
            let mut buffer = ReceiveBuffer::new();
            b.iter(move || {
                buffer.skip(black_box(size)).unwrap();
            });
        });

        group.bench_with_input(BenchmarkId::new("write_at", size), &input, |b, input| {
            let mut buffer = ReceiveBuffer::new();
            let mut offset = VarInt::from_u8(0);
            let len = VarInt::new(input.len() as _).unwrap();
            b.iter(move || {
                buffer.write_at(offset, input).unwrap();
                buffer.copy_into_buf(&mut NoOpBuf);
                offset += len;
            });
        });

        // we double the writes in the fragment test
        group.throughput(Throughput::Bytes((input.len() * 2) as _));
        group.bench_with_input(
            BenchmarkId::new("write_at_fragmented", size),
            &input,
            |b, input| {
                let mut buffer = ReceiveBuffer::new();
                let mut offset = VarInt::from_u8(0);
                let len = VarInt::new(input.len() as _).unwrap();
                b.iter(move || {
                    let first_offset = offset + len;
                    buffer.write_at(first_offset, input).unwrap();
                    let second_offset = offset;
                    buffer.write_at(second_offset, input).unwrap();
                    buffer.copy_into_buf(&mut NoOpBuf);
                    offset = first_offset + len;
                });
            },
        );
    }
}

/// A BufMut implementation that doesn't actually copy data into it
///
/// This is used to avoid oversampling the `pop` implementation for
/// `write_at` benchmarks.
struct NoOpBuf;

unsafe impl bytes::BufMut for NoOpBuf {
    #[inline]
    fn remaining_mut(&self) -> usize {
        usize::MAX
    }

    #[inline]
    unsafe fn advance_mut(&mut self, _cnt: usize) {}

    #[inline]
    fn put_slice(&mut self, _slice: &[u8]) {}

    #[inline]
    fn chunk_mut(&mut self) -> &mut bytes::buf::UninitSlice {
        todo!()
    }
}
