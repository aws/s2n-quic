// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

mod buffer;
mod frame;
mod inet;
mod packet;
mod packet_number_map;
mod sync;
mod varint;
mod xdp;

pub fn benchmarks(c: &mut Criterion) {
    buffer::benchmarks(c);
    frame::benchmarks(c);
    inet::benchmarks(c);
    packet::benchmarks(c);
    packet_number_map::benchmarks(c);
    sync::benchmarks(c);
    varint::benchmarks(c);
    xdp::benchmarks(c);
}
