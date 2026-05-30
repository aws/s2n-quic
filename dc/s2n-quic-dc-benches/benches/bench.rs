// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(not(feature = "cachegrind"))]
use criterion::{criterion_group, criterion_main};

#[cfg(not(feature = "cachegrind"))]
criterion_group!(benches, s2n_quic_dc_benches::benchmarks);
#[cfg(not(feature = "cachegrind"))]
criterion_main!(benches);

#[cfg(feature = "cachegrind")]
fn main() {
    // Stop instrumentation immediately so benchmark setup and library
    // initialisation are not counted.  Each run_bench call will start/stop
    // instrumentation around its own cold and warm runs.
    crabgrind::callgrind::stop_instrumentation();

    let mut c = s2n_quic_dc_benches::bench::Criterion::new();
    s2n_quic_dc_benches::benchmarks(&mut c);
}
