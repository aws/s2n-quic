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

use crate::{inet::SocketAddress, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Allow,
    Retry { delay: Duration },
    Drop,
    Close { delay: Duration },
}

#[non_exhaustive]
pub struct ConnectionInfo<'a> {
    #[allow(dead_code)]
    current_handshakes: usize,

    #[allow(dead_code)]
    source_address: &'a SocketAddress,
}

impl<'a> ConnectionInfo<'a> {
    pub fn new(current_handshakes: usize, source_address: &'a SocketAddress) -> Self {
        Self {
            current_handshakes,
            source_address,
        }
    }
}

pub trait Format {
    fn on_connection_attempt(&mut self, info: &ConnectionInfo) -> Outcome;
}
