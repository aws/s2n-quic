// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;

mod recv;
mod send;

pub fn benchmarks(c: &mut Criterion) {
    send::benches(c);
    recv::benches(c);
}
