// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    io::{rx, tx},
    time::Timestamp,
};
use core::{
    future::Future,
    task::{Context, Poll},
};

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

/// Enumerates endpoint locations
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Location {
    /// The local endpoint
    Local,
    /// The remote endpoint
    Remote,
}

impl Location {
    /// Returns true if the given endpoint is local
    pub fn is_local(self) -> bool {
        self == Self::Local
    }

    /// Returns true if the given endpoint is remote
    pub fn is_remote(self) -> bool {
        self == Self::Remote
    }

    /// Returns the [`Location`] of the peer.
    /// - If called on `Local` this will return `Remote`
    /// - If called on `Remote` this will return `Local`
    pub fn peer_type(self) -> Self {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }
}

/// The main interface for a QUIC endpoint
pub trait Endpoint: 'static + Send + Sized {
    /// Receives and processes datagrams for the Rx queue
    fn receive<'rx, Rx: rx::Rx<'rx>>(&mut self, rx: &'rx mut Rx, timestamp: Timestamp);

    /// Transmits outgoing datagrams into the Tx queue
    fn transmit<'tx, Tx: tx::Tx<'tx>>(&mut self, tx: &'tx mut Tx, timestamp: Timestamp);

    /// Returns a future which polls for application-space wakeups
    fn wakeups(&mut self, timestamp: Timestamp) -> Wakeups<Self> {
        Wakeups {
            endpoint: self,
            timestamp,
        }
    }

    /// Polls for any application-space wakeups
    fn poll_wakeups(
        &mut self,
        cx: &mut Context<'_>,
        timestamp: Timestamp,
    ) -> Poll<Result<usize, CloseError>>;

    /// Returns the latest Timestamp at which `transmit` should be called
    fn timeout(&self) -> Option<Timestamp>;
}

/// A future which polls an endpoint for application-space wakeups
pub struct Wakeups<'a, E: Endpoint> {
    endpoint: &'a mut E,
    timestamp: Timestamp,
}

impl<'a, E: Endpoint> Future for Wakeups<'a, E> {
    type Output = Result<usize, CloseError>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let timestamp = self.timestamp;
        self.endpoint.poll_wakeups(cx, timestamp)
    }
}

/// Indicates the endpoint is no longer processing connections.
#[derive(Clone, Copy, Debug)]
pub struct CloseError;
