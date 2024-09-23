// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

pub mod encrypt;
pub mod hkdf;
pub mod hmac;

pub fn benchmarks(c: &mut Criterion) {
    encrypt::benchmarks(c);
    hkdf::benchmarks(c);
    hmac::benchmarks(c);
}
