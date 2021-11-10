// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

mod aes;
mod aesgcm;
mod ghash;

pub fn benchmarks(c: &mut Criterion) {
    aes::benchmarks(c);
    aesgcm::benchmarks(c);
    ghash::benchmarks(c);
}
