use crate::{inet::SocketAddress, time::Duration};

/// Enumerates endpoint types
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EndpointType {
    /// The endpoint is a client
    Client,
    /// The endpoint is a server
    Server,
}

impl EndpointType {
    /// Returns true if the given endpoint is a QUIC client
    pub fn is_client(self) -> bool {
        self == EndpointType::Client
    }

    /// Returns true if the given endpoint is a QUIC server
    pub fn is_server(self) -> bool {
        self == EndpointType::Server
    }

    /// Returns the [`EndpointType`] of the peer.
    /// - If called on `Client` this will return `Server`
    /// - If called on `Server` this will return `Client`
    pub fn peer_type(self) -> EndpointType {
        match self {
            EndpointType::Client => EndpointType::Server,
            EndpointType::Server => EndpointType::Client,
        }
    }
}

/// A ConnectionAttempt holds information about the state of endpoint receiving a connect, along
/// with information about the connection. This can be used to make decisions about the Outcome of
/// an attempted connection
#[non_exhaustive]
pub struct ConnectionAttempt<'a> {
    /// Number of handshakes the have begun but not completed
    #[allow(dead_code)]
    inflight_handshakes: usize,

    /// The unverified address of the connecting peer
    /// This address comes from the datagram
    #[allow(dead_code)]
    source_address: &'a SocketAddress,
}

/// Outcome describes how the library should proceed on a connection attempt. The implementor will
/// use information from the ConnectionAttempt object to determine how the library should handle
/// the connection attempt
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Allow the connection to continue
    Allow,

    /// Defer the connection by sending a Retry packet after a `delay`
    Retry { delay: Duration },

    /// Silently drop the connection attempt
    Drop,

    /// Cleanly close the connection after a `delay`
    Close { delay: Duration },
}

impl<'a> ConnectionAttempt<'a> {
    pub fn new(inflight_handshakes: usize, source_address: &'a SocketAddress) -> Self {
        Self {
            inflight_handshakes,
            source_address,
        }
    }
}

/// This trait is used to determine the outcome of connection attempts on an endpoint. The
/// implementor returns an Outcome based on the ConnectionAttempt, or other information that the
/// implementor may have
/// ```rust,no_run
/// impl endpoint::Limits for MyEndpointLimits {
///     fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
///         if info.inflight_handshakes > my_server_limit {
///             return Outcome::Retry { delay: Duration::from_millis(current_backoff) }
///         }
///     }
/// }
pub trait Limits {
    fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome;
}
