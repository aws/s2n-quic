// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

pub mod crypto;
pub mod datagram;

pub fn benchmarks(c: &mut Criterion) {
    crypto::benchmarks(c);
    datagram::benchmarks(c);
}
