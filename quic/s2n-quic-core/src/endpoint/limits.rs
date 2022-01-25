// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{api::SocketAddress, IntoEvent},
    inet,
    time::Duration,
};

/// ConnectionAttemptOutcome describes how the library should proceed on a connection attempt. The
/// implementor will use information from the Context object to determine how the library should handle
/// the connection attempt
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionAttemptOutcome {
    /// Allow the connection to continue
    Allow,

    /// Defer the connection by sending a Retry packet after a `delay`
    Retry { delay: Duration },

    /// Silently drop the connection attempt
    Drop,

    /// Cleanly close the connection after a `delay`
    Close { delay: Duration },
}

/// LimitViolationOutcome describes how the library should proceed when an endpoint limit has been
/// violated. The implementor will use information from the Context object to determine how the library
/// should handle the connection that exceeded the limit.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LimitViolationOutcome {
    /// Ignore the violation and allow the connection to remain open
    Ignore,

    /// Close the connection immediately
    Close,
}

/// This context holds information about the state of endpoint along with information about the connection.
/// This can be used to make decisions about the Outcome of an attempted connection or end point limit
/// being exceeded.
#[non_exhaustive]
#[derive(Debug)]
pub struct Context<'a> {
    /// Number of handshakes the have begun but not completed
    pub inflight_handshakes: usize,

    /// Number of open connections
    pub connection_count: usize,

    /// The address of the peer. In the case of `on_connection_attempt`,
    /// this address has not yet been verified.
    pub remote_address: SocketAddress<'a>,
}

impl<'a> Context<'a> {
    #[doc(hidden)]
    pub fn new(
        inflight_handshakes: usize,
        connection_count: usize,
        remote_address: &'a inet::SocketAddress,
    ) -> Self {
        Self {
            inflight_handshakes,
            connection_count,
            remote_address: remote_address.into_event(),
        }
    }
}

/// This trait is used to determine the outcome of connection attempts on an endpoint. The
/// implementor returns an Outcome based on the ConnectionAttempt, or other information that the
/// implementor may have.
///
/// ```rust
/// # mod s2n_quic { pub mod provider { pub mod endpoint_limits { pub use s2n_quic_core::endpoint::limits::*; } } }
/// use s2n_quic::provider::endpoint_limits::{Limiter, Context, ConnectionAttemptOutcome, LimitViolationOutcome};
///
/// struct MyEndpointLimits {
///    handshake_limit: usize,
///    connection_limit: usize,
///    min_transfer_bytes_per_second: usize,
///    delay: core::time::Duration,
/// }
///
/// impl Limiter for MyEndpointLimits {
///    fn on_connection_attempt(&mut self, info: &Context) -> ConnectionAttemptOutcome {
///        if info.inflight_handshakes > self.handshake_limit {
///            ConnectionAttemptOutcome::Retry { delay: self.delay }
///        } else {
///            ConnectionAttemptOutcome::Allow
///        }
///    }
///
///    fn on_min_transfer_rate_violation(&mut self, context: &Context) -> LimitViolationOutcome {
///         if context.connection_count > self.connection_limit {
///            LimitViolationOutcome::Close
///         } else {
///            LimitViolationOutcome::Ignore
///         }
///    }
///
///    fn min_transfer_bytes_per_second(&self) -> usize {
///         self.min_transfer_bytes_per_second
///    }
/// }
/// ```
pub trait Limiter: 'static + Send {
    fn on_connection_attempt(&mut self, info: &Context) -> ConnectionAttemptOutcome;

    fn on_min_transfer_rate_violation(&mut self, context: &Context) -> LimitViolationOutcome;

    fn min_transfer_bytes_per_second(&self) -> usize;
}
