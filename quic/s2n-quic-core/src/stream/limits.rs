// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// The default send buffer size for Streams
const DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE: u32 = 64 * 1024;

/// Per-stream limits
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The maximum send buffer size for a Stream
    pub max_send_buffer_size: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

impl Limits {
    pub const RECOMMENDED: Self = Self {
        max_send_buffer_size: DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE,
    };
}
