// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("application:write")]
pub struct ApplicationWrite {
    /// The number of bytes that the application tried to write
    len: usize,
}

#[event("application:write")]
pub struct ApplicationRead {
    /// The number of bytes that the application tried to read
    len: usize,
}
