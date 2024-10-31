// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("application:write")]
pub struct ApplicationWrite {
    /// The number of bytes that the application tried to write
    #[measure("bytes.provided", "b")]
    total_len: usize,

    /// The amount that was written
    #[measure("bytes", "b")]
    #[counter("bytes.total", "b")]
    write_len: usize,
}

#[event("application:read")]
pub struct ApplicationRead {
    /// The number of bytes that the application tried to read
    #[measure("bytes.capacity", "b")]
    capacity: usize,

    /// The amount that was read
    #[measure("bytes", "b")]
    #[counter("bytes.total", "b")]
    read_len: usize,
}
