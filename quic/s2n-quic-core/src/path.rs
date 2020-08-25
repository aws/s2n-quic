//! This module contains the Path implementation

use crate::{connection::ConnectionId, inet::SocketAddress};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Validated,
    Pending { tx_bytes: u32, rx_bytes: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct Path {
    /// The peer's socket address
    pub peer_socket_address: SocketAddress,
    /// The connection id of the peer
    pub source_connection_id: ConnectionId,
    /// The the connection id the peer wanted to access
    pub destination_connection_id: ConnectionId,
    /// Tracks whether this path has passed Address or Path validation
    state: State,
}

/// A Path holds the local and peer socket addresses, connection ids, and state. It can be
/// validated or pending validation.
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
            state: State::Pending {
                tx_bytes: 0,
                rx_bytes: 0,
            },
        }
    }

    /// Called when bytes have been transmitted on this path
    pub fn on_bytes_transmitted(&mut self, bytes: u32) {
        if let State::Pending { tx_bytes, .. } = &mut self.state {
            *tx_bytes += bytes;
        }
    }

    /// Called when bytes have been received on this path
    pub fn on_bytes_received(&mut self, bytes: u32) {
        if let State::Pending { rx_bytes, .. } = &mut self.state {
            *rx_bytes += bytes;
        }
    }

    /// Called when the path is validated
    pub fn on_validated(&mut self) {
        self.state = State::Validated
    }

    /// Returns whether this path has passed address validation
    pub fn is_validated(&self) -> bool {
        self.state == State::Validated
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29#section-8.1
    //# Prior to validating the client address, servers MUST NOT send more
    //# than three times as many bytes as the number of bytes they have
    //# received.
    pub fn mtu(&self, requested_size: usize) -> usize {
        match self.state {
            State::Validated => requested_size,
            State::Pending { tx_bytes, rx_bytes } => {
                let limit = (rx_bytes * 3) - tx_bytes;
                requested_size.min(limit as usize)
            }
        }
    }

    /// Returns whether this path is blocked from transmitting more data
    pub fn at_amplification_limit(&self, min_packet_size: usize) -> bool {
        self.mtu(min_packet_size) < min_packet_size
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
        assert_eq!(path.at_amplification_limit(9), true);

        path.on_bytes_received(3);
        assert_eq!(path.at_amplification_limit(9), false);

        path.on_validated();
        path.on_bytes_transmitted(24);
        // Validated paths should always be able to transmit
        assert_eq!(path.at_amplification_limit(9), false);
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

        path.on_validated();
        // Validated paths should always be able to transmit
        assert_eq!(path.mtu(4), 4);
    }
}
