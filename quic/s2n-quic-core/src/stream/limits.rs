// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    transport::parameters::{InitialMaxStreamsBidi, InitialMaxStreamsUni, ValidationError},
    varint::VarInt,
};

// TODO investigate a good default
/// The default send buffer size for Streams
const DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE: u32 = 512 * 1024;

/// Per-stream limits
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The maximum send buffer size for a Stream
    pub max_send_buffer_size: LocalMaxSendBufferSize,
    /// The maximum number of unidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_unidirectional_streams: LocalUniDirectionalLimit,
    /// The maximum number of bidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_bidirectional_streams: LocalBiDirectionalLimit,
}

impl Default for Limits {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

impl Limits {
    pub const RECOMMENDED: Self = Self {
        max_send_buffer_size: LocalMaxSendBufferSize::RECOMMENDED,
        max_open_local_unidirectional_streams: LocalUniDirectionalLimit::RECOMMENDED,
        max_open_local_bidirectional_streams: LocalBiDirectionalLimit::RECOMMENDED,
    };
}

macro_rules! local_limits {
    ($name:ident($encodable_type:ty)) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $name($encodable_type);
    };
}

// VarInt specific functionality
macro_rules! varint_local_limits {
    ($name:ident($encodable_type:ty)) => {
        local_limits!($name($encodable_type));

        impl $name {
            pub const fn as_varint(&self) -> VarInt {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = ValidationError;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                let value = VarInt::new(value)?;
                Ok(Self(value))
            }
        }
    };
}

local_limits!(LocalMaxSendBufferSize(u32));

impl LocalMaxSendBufferSize {
    pub const RECOMMENDED: Self = Self(DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE);

    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for LocalMaxSendBufferSize {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

varint_local_limits!(LocalUniDirectionalLimit(VarInt));

impl LocalUniDirectionalLimit {
    pub const RECOMMENDED: Self = Self(InitialMaxStreamsUni::RECOMMENDED.as_varint());
}

varint_local_limits!(LocalBiDirectionalLimit(VarInt));

impl LocalBiDirectionalLimit {
    pub const RECOMMENDED: Self = Self(InitialMaxStreamsBidi::RECOMMENDED.as_varint());
}

// To maintain backwards API compatibility we need to convert from
// `max_open_bidirectional_streams` to `max_open_local_bidirectional_streams`
impl From<InitialMaxStreamsBidi> for LocalBiDirectionalLimit {
    fn from(value: InitialMaxStreamsBidi) -> Self {
        Self(value.as_varint())
    }
}
