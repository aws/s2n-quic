// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("application:write")]
pub struct ApplicationWrite {
    /// The number of bytes that the application tried to write
    #[measure("provided", "b")]
    total_len: usize,

    /// The amount that was written
    #[measure("committed", "b")]
    #[counter("committed.total", "b")]
    write_len: usize,
}

#[event("application:read")]
pub struct ApplicationRead {
    /// The number of bytes that the application tried to read
    #[measure("capacity", "b")]
    capacity: usize,

    /// The amount that was read
    #[measure("committed", "b")]
    #[counter("committed.total", "b")]
    read_len: usize,
}
