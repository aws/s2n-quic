// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_quic_dc::credentials::testing::iter as creds;

pub mod decrypt;
pub mod encrypt;
pub mod hkdf;

pub fn benchmarks(c: &mut Criterion) {
    encrypt::benchmarks(c);
    hkdf::benchmarks(c);
}
