// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::bench::Criterion;

pub mod bench;
pub mod crypto;
pub mod datagram;
pub mod endpoint;

pub fn benchmarks(c: &mut Criterion) {
    crypto::benchmarks(c);
    datagram::benchmarks(c);
    endpoint::benchmarks(c);
}
