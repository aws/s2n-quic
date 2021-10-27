// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

mod frame;
mod packet;
mod varint;

pub fn benchmarks(c: &mut Criterion) {
    frame::benchmarks(c);
    packet::benchmarks(c);
    varint::benchmarks(c);
}
