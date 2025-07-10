// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion};
use s2n_codec::{DecoderBuffer, Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::varint::VarInt;
use std::hint::black_box;

pub fn benchmarks(c: &mut Criterion) {
    encode(c);
    decode(c);
}

#[inline(always)]
pub fn encode_n<const N: usize>(inputs: [VarInt; N], buffer: &mut [u8]) {
    let mut encoder = EncoderBuffer::new(buffer);
    for input in inputs {
        if input.encoding_size() > encoder.remaining_capacity() {
            break;
        }
        encoder.encode(&input);
    }
}

#[inline(never)]
pub fn encode_slice(input: VarInt, buffer: &mut [u8]) {
    encode_n([input], buffer);
}

#[inline(never)]
pub fn encode_array(input: VarInt, buffer: &mut [u8; 8]) {
    encode_n([input], buffer);
}

#[inline(never)]
pub fn encode_16_slice(input: VarInt, buffer: &mut [u8]) {
    encode_n([input; 16], buffer);
}

#[inline(never)]
pub fn encode_16_array(input: VarInt, buffer: &mut [u8; 8 * 16]) {
    encode_n([input; 16], buffer);
}

#[inline(always)]
pub fn decode_n<const N: usize>(buffer: &[u8]) -> Option<[VarInt; N]> {
    let mut buffer = DecoderBuffer::new(buffer);
    let mut out = [VarInt::default(); N];
    for out in out.iter_mut() {
        let (v, remaining) = buffer.decode::<VarInt>().ok()?;
        *out = v;
        buffer = remaining;
    }
    Some(out)
}

#[inline(never)]
pub fn decode_slice(buffer: &[u8]) -> Option<VarInt> {
    Some(decode_n::<1>(buffer)?[0])
}

#[inline(never)]
pub fn decode_array(buffer: &[u8; 8]) -> Option<VarInt> {
    Some(decode_n::<1>(buffer)?[0])
}

#[inline(never)]
pub fn decode_16_slice(buffer: &[u8]) -> Option<[VarInt; 16]> {
    decode_n(buffer)
}

#[inline(never)]
pub fn decode_16_array(buffer: &[u8; 8 * 16]) -> Option<[VarInt; 16]> {
    decode_n(buffer)
}

fn encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint/encode");
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61] {
        let i = VarInt::new(2u64.pow(i)).unwrap();

        group.bench_with_input(BenchmarkId::new("array", i), &i, |b, input| {
            let mut buffer = [0u8; 8];
            b.iter(|| {
                encode_array(black_box(*input), black_box(&mut buffer));
            });
        });
        group.bench_with_input(BenchmarkId::new("slice", i), &i, |b, input| {
            let mut buffer = [0u8; 8];
            b.iter(|| {
                encode_slice(black_box(*input), black_box(&mut buffer));
            });
        });
        group.bench_with_input(BenchmarkId::new("array_16", i), &i, |b, input| {
            let mut buffer = [0u8; 8 * 16];
            b.iter(|| {
                encode_16_array(black_box(*input), black_box(&mut buffer));
            });
        });
        group.bench_with_input(BenchmarkId::new("slice_16", i), &i, |b, input| {
            let mut buffer = [0u8; 8 * 16];
            b.iter(|| {
                encode_16_slice(black_box(*input), black_box(&mut buffer));
            });
        });
    }
    group.finish();
}

fn decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint/decode");
    for i in [0, 1, 5, 6, 13, 14, 29, 30, 61] {
        let i = VarInt::new(2u64.pow(i)).unwrap();

        group.bench_with_input(BenchmarkId::new("array", i), &i, |b, input| {
            let mut buffer = [0; 8];
            encode_array(black_box(*input), black_box(&mut buffer));
            b.iter(|| {
                black_box(decode_array(black_box(&buffer)));
            });
        });
        group.bench_with_input(BenchmarkId::new("slice", i), &i, |b, input| {
            let mut buffer = [0; 8];
            encode_slice(black_box(*input), black_box(&mut buffer));
            b.iter(|| {
                black_box(decode_slice(black_box(&buffer)));
            });
        });
        group.bench_with_input(BenchmarkId::new("array_16", i), &i, |b, input| {
            let mut buffer = [0; 8 * 16];
            encode_16_array(black_box(*input), black_box(&mut buffer));
            b.iter(|| {
                black_box(decode_16_array(black_box(&buffer)));
            });
        });
        group.bench_with_input(BenchmarkId::new("slice_16", i), &i, |b, input| {
            let mut buffer = [0; 8 * 16];
            encode_16_slice(black_box(*input), black_box(&mut buffer));
            b.iter(|| {
                black_box(decode_16_slice(black_box(&buffer)));
            });
        });
    }
    group.finish();
}
