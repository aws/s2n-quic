// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{api::SocketAddress, IntoEvent, Timestamp},
    inet,
};

/// Outcome describes how the library should proceed on a connection attempt. The implementor will
/// use information from the ConnectionAttempt object to determine how the library should handle
/// the connection attempt
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Allow the connection to continue
    ///
    /// Use `Outcome::allow()` to construct this variant
    #[non_exhaustive]
    Allow,

    /// Defer the connection by sending a Retry packet
    ///
    /// Use `Outcome::retry()` to construct this variant
    #[non_exhaustive]
    Retry,

    /// Silently drop the connection attempt
    ///
    /// Use `Outcome::drop()` to construct this variant
    #[non_exhaustive]
    Drop,

    /// Cleanly close the connection
    ///
    /// Use `Outcome::close()` to construct this variant
    #[non_exhaustive]
    Close,
}

impl Outcome {
    /// Allow the connection to continue
    pub fn allow() -> Self {
        Self::Allow
    }

    /// Defer the connection by sending a Retry packet
    pub fn retry() -> Self {
        Self::Retry
    }

    /// Silently drop the connection attempt
    pub fn drop() -> Self {
        Self::Drop
    }

    /// Cleanly close the connection
    pub fn close() -> Self {
        Self::Close
    }
}

/// A ConnectionAttempt holds information about the state of endpoint receiving a connect, along
/// with information about the connection. This can be used to make decisions about the Outcome of
/// an attempted connection
#[non_exhaustive]
#[derive(Debug)]
pub struct ConnectionAttempt<'a> {
    /// Number of handshakes the have begun but not completed
    pub inflight_handshakes: usize,

    /// Number of open connections
    pub connection_count: usize,

    /// The unverified address of the connecting peer
    /// This address comes from the datagram
    pub remote_address: SocketAddress<'a>,
    pub timestamp: Timestamp,
}

impl<'a> ConnectionAttempt<'a> {
    #[doc(hidden)]
    pub fn new(
        inflight_handshakes: usize,
        connection_count: usize,
        remote_address: &'a inet::SocketAddress,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            inflight_handshakes,
            connection_count,
            remote_address: remote_address.into_event(),
            timestamp,
        }
    }
}

pub trait Limiter: 'static + Send {
    /// This trait is used to determine the outcome of connection attempts on an endpoint. The
    /// implementor returns an Outcome based on the ConnectionAttempt, or other information that the
    /// implementor may have.
    ///
    /// ```rust
    /// # mod s2n_quic { pub mod provider { pub mod endpoint_limits { pub use s2n_quic_core::endpoint::limits::*; } } }
    /// use s2n_quic::provider::endpoint_limits::{Limiter, ConnectionAttempt, Outcome};
    ///
    /// struct MyEndpointLimits {
    ///    handshake_limit: usize,
    ///    delay: core::time::Duration,
    /// }
    ///
    /// impl Limiter for MyEndpointLimits {
    ///    fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
    ///        if info.inflight_handshakes > self.handshake_limit {
    ///            Outcome::retry()
    ///        } else {
    ///            Outcome::allow()
    ///        }
    ///    }
    /// }
    /// ```
    fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome;
}
