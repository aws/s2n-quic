//! This module contains the Path implementation

use crate::{
    connection,
    frame::path_challenge,
    inet::SocketAddress,
    recovery::{CongestionController, RTTEstimator},
    transmission,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Validated,
    Pending { tx_bytes: u32, rx_bytes: u32 },
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#14.2
//# If a QUIC endpoint determines that the PMTU between any pair of local
//# and remote IP addresses has fallen below the smallest allowed maximum
//# packet size of 1200 bytes, it MUST immediately cease sending QUIC
//# packets
pub const MINIMUM_MTU: u16 = 1200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Path<CC: CongestionController> {
    /// The peer's socket address
    pub peer_socket_address: SocketAddress,
    /// The connection id of the peer
    pub peer_connection_id: connection::Id,
    /// The path owns the roundtrip between peers
    pub rtt_estimator: RTTEstimator,
    /// The congestion controller for the path
    pub congestion_controller: CC,
    /// Tracks whether this path has passed Address or Path validation
    state: State,
    /// Maximum transmission unit of the path
    mtu: u16,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.3
    //# To initiate path validation, an endpoint sends a PATH_CHALLENGE frame
    //# containing a random payload on the path to be validated.
    pub challenge: Option<[u8; path_challenge::DATA_LEN]>,

    /// True if the path has been validated by the peer
    peer_validated: bool,
}

/// A Path holds the local and peer socket addresses, connection ids, and state. It can be
/// validated or pending validation.
impl<CC: CongestionController> Path<CC> {
    pub fn new(
        peer_socket_address: SocketAddress,
        peer_connection_id: connection::Id,
        rtt_estimator: RTTEstimator,
        congestion_controller: CC,
        peer_validated: bool,
    ) -> Self {
        Path {
            peer_socket_address,
            peer_connection_id,
            rtt_estimator,
            congestion_controller,
            state: State::Pending {
                tx_bytes: 0,
                rx_bytes: 0,
            },
            mtu: MINIMUM_MTU,
            challenge: None,
            peer_validated,
        }
    }

    /// Called when bytes have been transmitted on this path
    pub fn on_bytes_transmitted(&mut self, bytes: usize) {
        if let State::Pending { tx_bytes, .. } = &mut self.state {
            *tx_bytes += bytes as u32;
        }
    }

    /// Called when bytes have been received on this path
    pub fn on_bytes_received(&mut self, bytes: usize) {
        if let State::Pending { rx_bytes, .. } = &mut self.state {
            *rx_bytes += bytes as u32;
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

    /// Marks the path as peer validated
    pub fn on_peer_validated(&mut self) {
        self.peer_validated = true;
    }

    /// Returns whether this path has been validated by the peer
    pub fn is_peer_validated(&self) -> bool {
        self.peer_validated
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1
    //# Prior to validating the client address, servers MUST NOT send more
    //# than three times as many bytes as the number of bytes they have
    //# received.
    pub fn clamp_mtu(&self, requested_size: usize) -> usize {
        match self.state {
            State::Validated => requested_size.min(self.mtu as usize),
            State::Pending { tx_bytes, rx_bytes } => {
                let limit = rx_bytes
                    .checked_mul(3)
                    .and_then(|v| v.checked_sub(tx_bytes))
                    .unwrap_or(0);
                requested_size.min(limit as usize).min(self.mtu as usize)
            }
        }
    }

    pub fn transmission_constraint(&self) -> transmission::Constraint {
        if self.at_amplification_limit() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
            //# Prior to validating the client address, servers MUST NOT send more
            //# than three times as many bytes as the number of bytes they have
            //# received.
            transmission::Constraint::AmplificationLimited
        } else if self.congestion_controller.requires_fast_retransmission() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
            //# If the congestion window is reduced immediately, a
            //# single packet can be sent prior to reduction.  This speeds up loss
            //# recovery if the data in the lost packet is retransmitted and is
            //# similar to TCP as described in Section 5 of [RFC6675].
            transmission::Constraint::RetransmissionOnly
        } else if self.congestion_controller.is_congestion_limited() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7
            //# An endpoint MUST NOT send a packet if it would cause bytes_in_flight
            //# (see Appendix B.2) to be larger than the congestion window, unless
            //# the packet is sent on a PTO timer expiration (see Section 6.2) or
            //# when entering recovery (see Section 7.3.2).
            transmission::Constraint::CongestionLimited
        } else {
            transmission::Constraint::None
        }
    }

    /// Returns whether this path is blocked from transmitting more data
    pub fn at_amplification_limit(&self) -> bool {
        let mtu = self.mtu as usize;
        self.clamp_mtu(mtu) < mtu
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::congestion_controller::testing::Unlimited;
    use core::time::Duration;

    #[test]
    fn amplification_limit_test() {
        let mut path = Path::new(
            SocketAddress::default(),
            connection::Id::try_from_bytes(&[]).unwrap(),
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            true,
        );

        // Verify we enforce the amplification limit if we can't send
        // at least 1 minimum sized packet
        path.on_bytes_received(1200);
        path.on_bytes_transmitted((1200 * 2) + 1);
        assert_eq!(path.at_amplification_limit(), true);

        path.on_bytes_received(1200);
        assert_eq!(path.at_amplification_limit(), false);

        path.on_validated();
        path.on_bytes_transmitted(24);
        // Validated paths should always be able to transmit
        assert_eq!(path.at_amplification_limit(), false);
    }

    #[test]
    fn mtu_test() {
        // TODO this would work better as a fuzz test
        let mut path = Path::new(
            SocketAddress::default(),
            connection::Id::try_from_bytes(&[]).unwrap(),
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            true,
        );

        path.on_bytes_received(3);
        path.on_bytes_transmitted(8);

        // Verify we can transmit one more byte
        assert_eq!(path.clamp_mtu(1), 1);
        assert_eq!(path.clamp_mtu(10), 1);

        path.on_bytes_transmitted(1);
        // Verify we can't transmit any more bytes
        assert_eq!(path.clamp_mtu(1), 0);
        assert_eq!(path.clamp_mtu(10), 0);

        path.on_bytes_received(1);
        // Verify we can transmit up to 3 more bytes
        assert_eq!(path.clamp_mtu(1), 1);
        assert_eq!(path.clamp_mtu(2), 2);
        assert_eq!(path.clamp_mtu(4), 3);

        path.on_validated();
        // Validated paths should always be able to transmit
        assert_eq!(path.clamp_mtu(4), 4);
    }

    #[test]
    fn peer_validated_test() {
        let mut path = Path::new(
            SocketAddress::default(),
            connection::Id::try_from_bytes(&[]).unwrap(),
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            false,
        );

        assert!(!path.is_peer_validated());

        path.on_peer_validated();

        assert!(path.is_peer_validated());
    }
}
