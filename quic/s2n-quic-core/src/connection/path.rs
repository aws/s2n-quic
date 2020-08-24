//! This module contains the Path implementation

use crate::{connection::ConnectionId, inet::SocketAddress};

/// Maintain metrics for each connection
#[derive(Debug, Copy, Clone)]
pub struct ConnectionMetrics {
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        ConnectionMetrics {
            bytes_sent: 0,
            bytes_recv: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Path {
    /// The peer's socket address
    pub peer_socket_address: SocketAddress,
    /// The connection id of the peer
    pub source_connection_id: ConnectionId,
    /// The the connection id the peer wanted to access
    pub destination_connection_id: ConnectionId,
    /// Holds metrics about bytes sent to the peer and received from the peer
    metrics: ConnectionMetrics,
    /// Tracks whether this path has passed Address or Path validation
    validated: bool,
}

/// A Path holds the local and peer socket addresses, conneciton ids, and metrics. It can be
/// validated or not-yet-validated.
impl Path {
    pub fn new(
        destination_connection_id: ConnectionId,
        peer_socket_address: SocketAddress,
        source_connection_id: ConnectionId,
    ) -> Self {
        Path {
            peer_socket_address,
            source_connection_id,
            destination_connection_id,
            metrics: Default::default(),
            validated: false,
        }
    }

    /// Called when bytes have been transmitted on this path
    pub fn on_bytes_transmitted(&mut self, bytes: usize) {
        self.metrics.bytes_sent += bytes;
    }

    /// Called when bytes have been received on this path
    pub fn on_bytes_received(&mut self, bytes: usize) {
        self.metrics.bytes_recv += bytes;
    }

    /// Returns whether this path has passed address validation
    pub fn is_validated(&self) -> bool {
        self.validated
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29#section-8.1
    //# Prior to validating the client address, servers MUST NOT send more
    //# than three times as many bytes as the number of bytes they have
    //# received.
    pub fn mtu(&self, requested_size: usize) -> usize {
        if self.validated {
            return requested_size;
        }

        let limit = (self.metrics.bytes_recv * 3) - self.metrics.bytes_sent;
        if limit < requested_size {
            limit
        } else {
            requested_size
        }
    }

    /// Returns whether this path is blocked from transmitting more data
    pub fn at_amplification_limit(&self) -> bool {
        self.mtu(1) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplification_limit_test() {
        let mut path = Path::new(
            ConnectionId::try_from_bytes(&[]).unwrap(),
            SocketAddress::default(),
            ConnectionId::try_from_bytes(&[]).unwrap(),
        );

        path.on_bytes_received(3);
        path.on_bytes_transmitted(8);
        assert_eq!(path.at_amplification_limit(), false);

        path.on_bytes_transmitted(1);
        assert_eq!(path.at_amplification_limit(), true);

        path.validated = true;
        // Validated paths should always be able to transmit
        assert_eq!(path.at_amplification_limit(), false);
    }

    #[test]
    fn mtu_test() {
        // TODO this would work better as a fuzz test
        let mut path = Path::new(
            ConnectionId::try_from_bytes(&[]).unwrap(),
            SocketAddress::default(),
            ConnectionId::try_from_bytes(&[]).unwrap(),
        );

        path.on_bytes_received(3);
        path.on_bytes_transmitted(8);

        // Verify we can transmit one more byte
        assert_eq!(path.mtu(1), 1);
        assert_eq!(path.mtu(10), 1);

        path.on_bytes_transmitted(1);
        // Verify we can't transmit any more bytes
        assert_eq!(path.mtu(1), 0);
        assert_eq!(path.mtu(10), 0);

        path.on_bytes_received(1);
        // Verify we can transmit up to 3 more bytes
        assert_eq!(path.mtu(1), 1);
        assert_eq!(path.mtu(2), 2);
        assert_eq!(path.mtu(4), 3);

        path.validated = true;
        // Validated paths should always be able to transmit
        assert_eq!(path.mtu(4), 4);
    }
}
