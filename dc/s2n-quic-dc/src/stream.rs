// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;

/// The maximum time a stream will be open without activity from the peer
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// The maximum length of a single packet written to a stream
pub const MAX_DATAGRAM_SIZE: usize = 1 << 15; // 32k

pub use crate::stream::socket::Protocol;

pub mod application;
pub mod client;
pub mod crypto;
pub mod endpoint;
pub mod environment;
pub mod pacer;
pub mod packet_map;
pub mod packet_number;
pub mod processing;
pub mod recv;
pub mod runtime;
pub mod send;
pub mod server;
pub mod shared;
pub mod socket;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TransportFeatures: u8 {
        /// The underlying transport guarantees transmission
        const RELIABLE = 1;
        /// The underlying transport provides flow control
        const FLOW_CONTROL = 2;
        /// The underlying transport provides stream abstractions
        const STREAM = 3;
        /// The underlying transport provides connections between peers
        const CONNECTED = 4;
    }
}

impl Default for TransportFeatures {
    #[inline]
    fn default() -> Self {
        TransportFeatures::empty()
    }
}

macro_rules! is_feature {
    ($is_feature:ident, $NAME:ident) => {
        #[inline]
        pub const fn $is_feature(&self) -> bool {
            self.contains(Self::$NAME)
        }
    };
}

impl TransportFeatures {
    pub const TCP: Self = Self::all();
    pub const UDP: Self = Self::empty();

    is_feature!(is_reliable, RELIABLE);
    is_feature!(is_flow_controlled, FLOW_CONTROL);
    is_feature!(is_stream, STREAM);
    is_feature!(is_connected, CONNECTED);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    Application,
    Worker,
}
