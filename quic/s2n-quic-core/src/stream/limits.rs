// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    transport::parameters::{
        InitialMaxStreamsBidi, InitialMaxStreamsUni, ValidationError, MAX_STREAMS_MAX_VALUE,
    },
    varint::VarInt,
};

/// The default send buffer size for Streams
///
/// This value is based on a common default maximum send buffer size for TCP (net.ipv4.tcp_wmem)
const DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE: u32 = 4096 * 1024;

pub trait LocalLimits {
    fn as_varint(&self) -> VarInt;
}

/// Per-stream limits
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Limits {
    /// The maximum send buffer size for a Stream
    pub max_send_buffer_size: MaxSendBufferSize,
    /// The maximum number of unidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_unidirectional_streams: LocalUnidirectional,
    /// The maximum number of bidirectional streams that may
    /// be opened concurrently by the local endpoint. This value
    /// is not communicated to the peer, it is only used for limiting
    /// concurrent streams opened locally by the application.
    pub max_open_local_bidirectional_streams: LocalBidirectional,
    /// Maximum cumulative number of bidirectional streams the remote peer may open.
    pub max_total_remote_bidirectional_streams: TotalRemoteBidirectional,
    /// Maximum cumulative number of unidirectional streams the remote peer may open.
    pub max_total_remote_unidirectional_streams: TotalRemoteUnidirectional,
}

impl Default for Limits {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

impl Limits {
    pub const RECOMMENDED: Self = Self {
        max_send_buffer_size: MaxSendBufferSize::RECOMMENDED,
        max_open_local_unidirectional_streams: LocalUnidirectional::RECOMMENDED,
        max_open_local_bidirectional_streams: LocalBidirectional::RECOMMENDED,
        max_total_remote_bidirectional_streams: TotalRemoteBidirectional::RECOMMENDED,
        max_total_remote_unidirectional_streams: TotalRemoteUnidirectional::RECOMMENDED,
    };
}

macro_rules! limits {
    ($name:ident($encodable_type:ty)) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub struct $name($encodable_type);
    };
}

// VarInt-specific functionality. An optional `$max` bounds accepted values.
macro_rules! varint_limits {
    ($name:ident($encodable_type:ty) $(, $max:expr)?) => {
        limits!($name($encodable_type));

        impl TryFrom<u64> for $name {
            type Error = ValidationError;

            #[inline]
            fn try_from(value: u64) -> Result<Self, Self::Error> {
                let value = VarInt::new(value)?;
                $(
                    if value > $max {
                        return Err(ValidationError(concat!(
                            stringify!($name),
                            " exceeds its maximum value"
                        )));
                    }
                )?
                Ok(Self(value))
            }
        }
    };
}

// Limits for streams opened locally.
macro_rules! varint_local_limits {
    ($name:ident($encodable_type:ty)) => {
        varint_limits!($name($encodable_type));

        impl LocalLimits for $name {
            #[inline]
            fn as_varint(&self) -> VarInt {
                self.0
            }
        }
    };
}

// Limits for streams opened by the peer.
macro_rules! varint_remote_limits {
    ($name:ident($encodable_type:ty), $max:expr) => {
        varint_limits!($name($encodable_type), $max);

        impl $name {
            #[inline]
            pub const fn as_varint(self) -> VarInt {
                self.0
            }
        }
    };
}

limits!(MaxSendBufferSize(u32));

impl MaxSendBufferSize {
    pub const RECOMMENDED: Self = Self(DEFAULT_STREAM_MAX_SEND_BUFFER_SIZE);

    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for MaxSendBufferSize {
    type Error = ValidationError;

    #[inline]
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

varint_local_limits!(LocalUnidirectional(VarInt));

impl LocalUnidirectional {
    pub const RECOMMENDED: Self = Self(InitialMaxStreamsUni::RECOMMENDED.as_varint());
}

varint_local_limits!(LocalBidirectional(VarInt));

impl LocalBidirectional {
    pub const RECOMMENDED: Self = Self(InitialMaxStreamsBidi::RECOMMENDED.as_varint());
}

impl From<InitialMaxStreamsBidi> for LocalBidirectional {
    fn from(value: InitialMaxStreamsBidi) -> Self {
        Self(value.as_varint())
    }
}

// Cumulative remote stream limits are bounded by `MAX_STREAMS_MAX_VALUE`, the
// largest cumulative count a MAX_STREAMS frame can carry.
varint_remote_limits!(TotalRemoteUnidirectional(VarInt), MAX_STREAMS_MAX_VALUE);

impl TotalRemoteUnidirectional {
    pub const RECOMMENDED: Self = Self(MAX_STREAMS_MAX_VALUE);
}

varint_remote_limits!(TotalRemoteBidirectional(VarInt), MAX_STREAMS_MAX_VALUE);

impl TotalRemoteBidirectional {
    pub const RECOMMENDED: Self = Self(MAX_STREAMS_MAX_VALUE);
}
