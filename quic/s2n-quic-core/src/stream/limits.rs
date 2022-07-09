// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{transport::parameters::InitialMaxStreamsUni, varint::VarInt};

// TODO investigate a good default
/// The default send buffer size for Streams
const DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE: u32 = 512 * 1024;

/// Per-stream limits
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The maximum send buffer size for a Stream
    pub max_send_buffer_size: u32,
    /// The maximum number of unidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_unidirectional_streams: VarInt,
    /// The maximum number of bidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_bidirectional_streams: VarInt,
}

impl Default for Limits {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

impl Limits {
    pub const RECOMMENDED: Self = Self {
        max_send_buffer_size: DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE,
        max_open_local_unidirectional_streams: InitialMaxStreamsUni::RECOMMENDED.as_varint(),
        max_open_local_bidirectional_streams: InitialMaxStreamsUni::RECOMMENDED.as_varint(),
    };
}
