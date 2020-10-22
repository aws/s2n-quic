use crate::{inet::SocketAddress, time::Duration};

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

/// A ConnectionAttempt holds information about the state of endpoint receiving a connect, along
/// with information about the connection. This can be used to make decisions about the Outcome of
/// an attempted connection
#[non_exhaustive]
#[derive(Debug)]
pub struct ConnectionAttempt<'a> {
    /// Number of handshakes the have begun but not completed
    #[allow(dead_code)]
    inflight_handshakes: usize,

    /// The unverified address of the connecting peer
    /// This address comes from the datagram
    #[allow(dead_code)]
    source_address: &'a SocketAddress,
}

impl<'a> ConnectionAttempt<'a> {
    pub fn new(inflight_handshakes: usize, source_address: &'a SocketAddress) -> Self {
        Self {
            inflight_handshakes,
            source_address,
        }
    }
}
