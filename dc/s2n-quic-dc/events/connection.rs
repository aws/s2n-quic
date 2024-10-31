// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("application:write")]
pub struct ApplicationWrite {
    /// The number of bytes that the application tried to write
    total_len: usize,

    /// The amount that was written
    write_len: usize,
}

#[event("application:read")]
pub struct ApplicationRead {
    /// The number of bytes that the application tried to read
    capacity: usize,

    /// The amount that was read
    read_len: usize,
}
