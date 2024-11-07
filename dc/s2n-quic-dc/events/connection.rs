// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("application:write")]
pub struct ApplicationWrite {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    total_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    write_len: usize,
}

#[event("application:read")]
pub struct ApplicationRead {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount that was read
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    read_len: usize,
}
