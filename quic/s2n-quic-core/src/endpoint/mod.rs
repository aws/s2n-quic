pub mod limits;

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

/// This trait is used to determine the outcome of connection attempts on an endpoint. The
/// implementor returns an Outcome based on the ConnectionAttempt, or other information that the
/// implementor may have.
///
/// ```rust,ignore
///  impl endpoint::Limits for MyEndpointLimits {
///     fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
///         if info.inflight_handshakes > my_server_limit {
///             return Outcome::Retry { delay: Duration::from_millis(current_backoff) }
///         }
///     }
///  }
/// ```
pub trait Limits {
    fn on_connection_attempt(&mut self, info: &limits::ConnectionAttempt) -> limits::Outcome;
}
