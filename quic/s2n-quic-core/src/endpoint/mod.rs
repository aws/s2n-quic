// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    io::{rx, tx},
    path::{self, mtu},
    time::{Clock, Timestamp},
};
use core::{
    fmt,
    future::Future,
    task::{Context, Poll},
};

pub mod limits;
pub use limits::Limiter;

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
    #[must_use]
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

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Local => write!(f, "the local endpoint"),
            Self::Remote => write!(f, "the remote endpoint"),
        }
    }
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
    #[must_use]
    pub fn peer_type(self) -> Self {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }
}

/// The main interface for a QUIC endpoint
pub trait Endpoint: 'static + Send + Sized {
    type PathHandle: path::Handle;
    type Subscriber: crate::event::Subscriber;

    const ENDPOINT_TYPE: Type;

    /// Receives and processes datagrams for the Rx queue
    fn receive<Rx, C>(&mut self, rx: &mut Rx, clock: &C)
    where
        Rx: rx::Queue<Handle = Self::PathHandle>,
        C: Clock;

    /// Transmits outgoing datagrams into the Tx queue
    fn transmit<Tx, C>(&mut self, tx: &mut Tx, clock: &C)
    where
        Tx: tx::Queue<Handle = Self::PathHandle>,
        C: Clock;

    /// Returns a future which polls for application-space wakeups
    ///
    /// When successful, the number of wakeups is returned.
    fn wakeups<'a, C: Clock>(&'a mut self, clock: &'a C) -> Wakeups<'a, Self, C> {
        Wakeups {
            endpoint: self,
            clock,
        }
    }

    /// Polls for any application-space wakeups
    ///
    /// When successful, the number of wakeups is returned.
    fn poll_wakeups<C: Clock>(
        &mut self,
        cx: &mut Context<'_>,
        clock: &C,
    ) -> Poll<Result<usize, CloseError>>;

    /// Returns the latest Timestamp at which `transmit` should be called
    fn timeout(&self) -> Option<Timestamp>;

    /// Sets configuration for the maximum transmission unit (MTU) that can be sent on a path
    fn set_mtu_config(&mut self, mtu_config: mtu::Config);

    /// Returns the endpoint's event subscriber
    fn subscriber(&mut self) -> &mut Self::Subscriber;
}

/// A future which polls an endpoint for application-space wakeups
pub struct Wakeups<'a, E: Endpoint, C: Clock> {
    endpoint: &'a mut E,
    clock: &'a C,
}

impl<'a, E: Endpoint, C: Clock> Future for Wakeups<'a, E, C> {
    type Output = Result<usize, CloseError>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let clock = self.clock;
        self.endpoint.poll_wakeups(cx, clock)
    }
}

/// Indicates the endpoint is no longer processing connections.
#[derive(Clone, Copy, Debug)]
pub struct CloseError;
