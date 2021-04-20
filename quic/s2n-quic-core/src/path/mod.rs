// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Path implementation

pub mod challenge;

use crate::{
    connection,
    inet::SocketAddress,
    recovery::{CongestionController, RttEstimator},
    time::Timestamp,
    transmission,
};
use challenge::Challenge;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum State {
    /// Path has no transmission limitations
    Validated,

    /// Path has not been validated and is subject to amplification limits
    AmplificationLimited {
        tx_bytes: u32,
        rx_bytes: u32,
    },
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
    //= type=TODO
    //# The endpoint
    //# MUST NOT send more than a minimum congestion window's worth of data
    //# per estimated round-trip time (kMinimumWindow, as defined in
    //# [QUIC-RECOVERY]).
    PendingChallengeResponse,
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
//# The maximum datagram size MUST be at least 1200 bytes.
pub const MINIMUM_MTU: u16 = 1200;

// Initial PTO backoff multiplier is 1 indicating no additional increase to the backoff.
pub const INITIAL_PTO_BACKOFF: u32 = 1;

#[derive(Debug, Clone)]
pub struct Path<CC: CongestionController> {
    /// The peer's socket address
    pub peer_socket_address: SocketAddress,
    /// The connection id of the peer
    pub peer_connection_id: connection::PeerId,
    /// The path owns the roundtrip between peers
    pub rtt_estimator: RttEstimator,
    /// The congestion controller for the path
    pub congestion_controller: CC,
    /// Probe timeout backoff multiplier
    pub pto_backoff: u32,
    /// Tracks whether this path has passed Address or Path validation
    state: State,
    /// Maximum transmission unit of the path
    mtu: u16,

    /// True if the path has been validated by the peer
    peer_validated: bool,

    /// Challenge sent to the peer in a PATH_CHALLENGE
    challenge: Challenge,
}

/// A Path holds the local and peer socket addresses, connection ids, and state. It can be
/// validated or pending validation.
impl<CC: CongestionController> Path<CC> {
    pub fn new(
        peer_socket_address: SocketAddress,
        peer_connection_id: connection::PeerId,
        rtt_estimator: RttEstimator,
        congestion_controller: CC,
        peer_validated: bool,
    ) -> Path<CC> {
        Path {
            peer_socket_address,
            peer_connection_id,
            rtt_estimator,
            congestion_controller,
            pto_backoff: INITIAL_PTO_BACKOFF,
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.4
            //# If the client IP address has changed, the server MUST
            //# adhere to the anti-amplification limits found in Section 8.1.
            // Start each path in State::AmplificationLimited until it has been validated.
            state: State::AmplificationLimited {
                tx_bytes: 0,
                rx_bytes: 0,
            },
            mtu: MINIMUM_MTU,
            peer_validated,
            challenge: Challenge::None,
        }
    }

    pub fn with_challenge(mut self, challenge: Challenge) -> Self {
        self.challenge = challenge;
        self
    }

    /// Called when bytes have been transmitted on this path
    pub fn on_bytes_transmitted(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }

        debug_assert_ne!(
            self.clamp_mtu(bytes),
            0,
            "path should not transmit when amplification limited; tried to transmit {}",
            bytes
        );

        if let State::AmplificationLimited { tx_bytes, .. } = &mut self.state {
            *tx_bytes += bytes as u32;
        }
    }

    /// Called when bytes have been received on this path
    /// Returns true if receiving these bytes unblocked the
    /// path from being amplification limited
    pub fn on_bytes_received(&mut self, bytes: usize) -> bool {
        let was_at_amplification_limit = self.at_amplification_limit();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //# For the purposes of
        //# avoiding amplification prior to address validation, servers MUST
        //# count all of the payload bytes received in datagrams that are
        //# uniquely attributed to a single connection.
        if let State::AmplificationLimited { rx_bytes, .. } = &mut self.state {
            *rx_bytes += bytes as u32;
        }

        was_at_amplification_limit && !self.at_amplification_limit()
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        self.challenge.on_timeout(timestamp)
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.challenge.timers()
    }

    /// When transmitting on a path this handles any internal state operations.
    pub fn on_transmit(&mut self, timestamp: Timestamp) {
        self.challenge.on_transmit(timestamp)
    }

    pub fn challenge_data(&self) -> Option<&challenge::Data> {
        self.challenge.data()
    }

    pub fn is_challenge_pending(&self, timestamp: Timestamp) -> bool {
        self.challenge.is_pending(timestamp)
    }

    pub fn is_challenge_abandoned(&self) -> bool {
        if let Challenge::Abandoned = self.challenge {
            return true;
        }

        false
    }

    pub fn validate_path_response(&mut self, timestamp: Timestamp, response: &[u8]) {
        if self.challenge.is_valid(timestamp, response) {
            self.on_validated();

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
            //= type=TODO
            //# After verifying a new client address, the server SHOULD send new
            //# address validation tokens (Section 8) to the client.
        }
    }

    /// Called when the path is validated
    pub fn on_validated(&mut self) {
        self.challenge = Challenge::None;
        self.state = State::Validated;
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

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
    //# The server MUST also limit the number of bytes it sends before
    //# validating the address of the client; see Section 8.
    pub fn clamp_mtu(&self, requested_size: usize) -> usize {
        match self.state {
            State::Validated => requested_size.min(self.mtu as usize),

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
            //= type=TODO
            //# Until a peer's address is deemed valid, an endpoint MUST
            //# limit the rate at which it sends data to this address.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
            //= type=TODO
            //# The endpoint
            //# MUST NOT send more than a minimum congestion window's worth of data
            //# per estimated round-trip time (kMinimumWindow, as defined in
            //# [QUIC-RECOVERY]).
            State::PendingChallengeResponse => requested_size.min(self.mtu as usize),

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
            //# Prior to validating the client address, servers MUST NOT send more
            //# than three times as many bytes as the number of bytes they have
            //# received.
            State::AmplificationLimited { tx_bytes, rx_bytes } => {
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
        } else if self.congestion_controller.is_congestion_limited() {
            if self.congestion_controller.requires_fast_retransmission() {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
                //# If the congestion window is reduced immediately, a
                //# single packet can be sent prior to reduction.  This speeds up loss
                //# recovery if the data in the lost packet is retransmitted and is
                //# similar to TCP as described in Section 5 of [RFC6675].
                transmission::Constraint::RetransmissionOnly
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7
                //# An endpoint MUST NOT send a packet if it would cause bytes_in_flight
                //# (see Appendix B.2) to be larger than the congestion window, unless
                //# the packet is sent on a PTO timer expiration (see Section 6.2) or
                //# when entering recovery (see Section 7.3.2).
                transmission::Constraint::CongestionLimited
            }
        } else {
            transmission::Constraint::None
        }
    }

    /// Returns whether this path should be limited according to connection establishment amplification limits
    pub fn at_amplification_limit(&self) -> bool {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.
        let mtu = self.mtu as usize;
        self.clamp_mtu(mtu) < mtu
    }

    /// Returns the current PTO period
    pub fn pto_period(
        &self,
        space: crate::packet::number::PacketNumberSpace,
    ) -> core::time::Duration {
        self.rtt_estimator.pto_period(self.pto_backoff, space)
    }

    /// Resets the PTO backoff to the initial value
    pub fn reset_pto_backoff(&mut self) {
        self.pto_backoff = INITIAL_PTO_BACKOFF;
    }

    /// Marks the path as closing
    pub fn on_closing(&mut self) {
        // Revert the path state to AmplificationLimited so we can control the number
        // of packets sent back with anti-amplification limits
        match &self.state {
            // keep the current amplification limits
            State::AmplificationLimited { .. } => {}
            State::Validated | State::PendingChallengeResponse => {
                self.state = State::AmplificationLimited {
                    tx_bytes: 0,
                    rx_bytes: MINIMUM_MTU as _,
                };
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::recovery::congestion_controller::testing::unlimited::CongestionController as Unlimited;
    use core::time::Duration;

    pub fn test_path() -> Path<Unlimited> {
        Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            RttEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::testing::Subscriber,
        recovery::CubicCongestionController,
        time::{Clock, NoopClock},
    };
    use core::time::Duration;

    #[test]
    fn amplification_limit_test() {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.4
        //= type=test
        //# If the client IP address has changed, the server MUST
        //# adhere to the anti-amplification limits found in Section 8.1.
        // This is tested here by verifying a new Path starts in State::AmplificationLimited

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //= type=test
        //# For the purposes of
        //# avoiding amplification prior to address validation, servers MUST
        //# count all of the payload bytes received in datagrams that are
        //# uniquely attributed to a single connection.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.4
        //= type=test
        //# If the client IP address has changed, the server MUST
        //# adhere to the anti-amplification limits found in Section 8.1.
        // This tests the IP change because a new path is created when a new peer_address is
        // detected. This new path should always start in State::Pending.
        let mut path = testing::test_path();

        // Verify we enforce the amplification limit if we can't send
        // at least 1 minimum sized packet
        let mut unblocked = path.on_bytes_received(1200);
        assert!(unblocked);
        path.on_bytes_transmitted((1200 * 2) + 1);
        assert_eq!(path.at_amplification_limit(), true);
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::AmplificationLimited
        );

        unblocked = path.on_bytes_received(1200);
        assert_eq!(path.at_amplification_limit(), false);
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
        assert!(unblocked);

        path.on_bytes_transmitted((1200 * 6) + 1);
        assert_eq!(path.at_amplification_limit(), true);
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::AmplificationLimited
        );
        unblocked = path.on_bytes_received(1200);
        assert!(!unblocked);

        path.on_validated();
        path.on_bytes_transmitted(24);
        // Validated paths should always be able to transmit
        assert_eq!(path.at_amplification_limit(), false);
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );

        // If we were already not amplification limited, receiving
        // more bytes doesn't unblock
        unblocked = path.on_bytes_received(1200);
        assert!(!unblocked);
    }

    #[test]
    fn mtu_test() {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //= type=test
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
        //= type=test
        //# The server MUST also limit the number of bytes it sends before
        //# validating the address of the client; see Section 8.
        // TODO this would work better as a fuzz test
        let mut path = testing::test_path();

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
        let mut path = testing::test_path();
        path.peer_validated = false;

        assert!(!path.is_peer_validated());

        path.on_peer_validated();

        assert!(path.is_peer_validated());
    }

    #[test]
    fn transmission_constraint_test() {
        let mut path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            RttEstimator::new(Duration::from_millis(30)),
            CubicCongestionController::new(MINIMUM_MTU),
            false,
        );
        let now = NoopClock.get_time();
        path.on_validated();

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );

        // Fill up the congestion controller
        path.congestion_controller
            .on_packet_sent(now, path.congestion_controller.congestion_window() as usize);

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::CongestionLimited
        );

        // Lose a byte to enter recovery
        path.congestion_controller
            .on_packets_lost(1, false, now, &mut Subscriber);

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::RetransmissionOnly
        );

        // Lose remaining bytes
        path.congestion_controller.on_packets_lost(
            path.congestion_controller.congestion_window(),
            false,
            now,
            &mut Subscriber,
        );

        // Since we are no longer congestion limited, there is no transmission constraint
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
    }
}
