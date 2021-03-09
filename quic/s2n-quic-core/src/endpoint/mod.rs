// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod limits;
pub use limits::Limits;

/// Enumerates endpoint types
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    /// The endpoint is a client
    Client,
    /// The endpoint is a server
    Server,
}

impl Type {
    /// Returns true if the given endpoint is a QUIC client
    pub fn is_client(self) -> bool {
        self == Self::Client
    }

    /// Returns true if the given endpoint is a QUIC server
    pub fn is_server(self) -> bool {
        self == Self::Server
    }

    /// Returns the [`Type`] of the peer.
    /// - If called on `Client` this will return `Server`
    /// - If called on `Server` this will return `Client`
    pub fn peer_type(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }
}
