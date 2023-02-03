// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

mod buffer;
mod crypto;
mod frame;
mod packet;
mod sync;
mod varint;

pub fn benchmarks(c: &mut Criterion) {
    buffer::benchmarks(c);
    crypto::benchmarks(c);
    frame::benchmarks(c);
    packet::benchmarks(c);
    sync::benchmarks(c);
    varint::benchmarks(c);
}
