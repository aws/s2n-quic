// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{criterion_group, criterion_main};

criterion_group!(benches, s2n_quic_dc_benches::benchmarks);
criterion_main!(benches);
