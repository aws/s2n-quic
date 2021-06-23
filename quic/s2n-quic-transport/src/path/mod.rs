// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Path implementation
mod challenge;
mod manager;
pub(crate) mod mtu;

pub use challenge::*;
pub use manager::*;

/// re-export core
pub use s2n_quic_core::path::*;
use s2n_quic_core::{ack, frame, time::Timestamp};

use crate::{
    connection,
    connection::close::SocketAddress,
    contexts::WriteContext,
    recovery::{CongestionController, RttEstimator},
    transmission,
    transmission::Mode,
};

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum State {
    /// Path has no transmission limitations
    Validated,

    /// Path has not been validated and is subject to amplification limits
    AmplificationLimited { tx_bytes: u32, rx_bytes: u32 },
}

#[derive(Debug, Clone)]
pub struct Path<CC: CongestionController> {
    /// The peer's socket address
    pub peer_socket_address: SocketAddress,
    /// The connection id of the peer
    pub peer_connection_id: connection::PeerId,
    /// The local connection id which the peer sends to
    pub local_connection_id: connection::LocalId,
    /// The path owns the roundtrip between peers
    pub rtt_estimator: RttEstimator,
    /// The congestion controller for the path
    pub congestion_controller: CC,
    /// Probe timeout backoff multiplier
    pub pto_backoff: u32,
    /// Tracks whether this path has passed Address or Path validation
    state: State,
    /// Controller for determining the maximum transmission unit of the path
    pub mtu_controller: mtu::Controller,

    /// True if the path has been validated by the peer
    peer_validated: bool,

    /// Challenge sent to the peer in a PATH_CHALLENGE
    challenge: Option<Challenge>,
    /// Received a Challenge and should echo back data in PATH_RESPONSE
    response_data: Option<challenge::Data>,
}

/// A Path holds the local and peer socket addresses, connection ids, and state. It can be
/// validated or pending validation.
impl<CC: CongestionController> Path<CC> {
    pub fn new(
        peer_socket_address: SocketAddress,
        peer_connection_id: connection::PeerId,
        local_connection_id: connection::LocalId,
        rtt_estimator: RttEstimator,
        congestion_controller: CC,
        peer_validated: bool,
    ) -> Path<CC> {
        Path {
            peer_socket_address,
            peer_connection_id,
            local_connection_id,
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
            mtu_controller: mtu::Controller::new(DEFAULT_MAX_MTU, &peer_socket_address),
            peer_validated,
            challenge: None,
            response_data: None,
        }
    }

    pub fn with_challenge(mut self, challenge: Challenge) -> Self {
        self.challenge = Some(challenge);
        self
    }

    /// Called when bytes have been transmitted on this path
    pub fn on_bytes_transmitted(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }

        let is_validated = self.is_validated();
        if is_validated {
            debug_assert_ne!(
                self.clamp_mtu(bytes, transmission::Mode::Normal),
                0,
                "path should not transmit when amplification limited; tried to transmit {}",
                bytes
            );
        }

        if let State::AmplificationLimited { tx_bytes, rx_bytes } = &mut self.state {
            *tx_bytes = if is_validated {
                *tx_bytes + bytes as u32
            } else {
                // during path validation we ignore the limit and send the max mtu limit
                // to also do mtu validation.
                *rx_bytes
            };
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
        if let Some(challenge) = &mut self.challenge {
            challenge.on_timeout(timestamp);
            if challenge.is_abandoned() {
                self.challenge = None;
            }
        }
        self.mtu_controller.on_timeout(timestamp);
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        core::iter::empty()
            .chain(
                self.challenge
                    .as_ref()
                    .map(|challenge| challenge.timers())
                    .into_iter()
                    .flatten(),
            )
            .chain(self.mtu_controller.timers())
    }

    /// Called when packets are acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.mtu_controller
            .on_packet_ack(ack_set, &mut self.congestion_controller)
    }

    /// Called when packets are lost
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.mtu_controller.on_packet_loss(ack_set)
    }

    /// When transmitting on a path this handles any internal state operations.
    ///
    /// PATH_CHALLENGE and PATH_RESPONSE should be transmitted first here since
    /// those frames are prioritized to complete path validation.
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if let Some(response_data) = &mut self.response_data {
            let frame = frame::PathResponse {
                data: &response_data,
            };
            if context.write_frame(&frame).is_some() {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
                //# An endpoint MUST NOT send more than one PATH_RESPONSE frame in
                //# response to one PATH_CHALLENGE frame; see Section 13.3.
                self.response_data = None;
            }
        }

        if let Some(challenge) = &mut self.challenge {
            challenge.on_transmit(context)
        }
    }

    pub fn is_challenge_pending(&self) -> bool {
        if let Some(challenge) = &self.challenge {
            !challenge.is_abandoned()
        } else {
            false
        }
    }

    pub fn on_path_challenge(&mut self, response: &challenge::Data) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
        //# echoing the data contained in the PATH_CHALLENGE frame in a
        //# PATH_RESPONSE frame.
        self.response_data = Some(*response);
    }

    pub fn on_path_response(&mut self, response: &[u8]) {
        if let Some(challenge) = &self.challenge {
            if challenge.is_valid(response) {
                self.on_validated();

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
                //= type=TODO
                //# After verifying a new client address, the server SHOULD send new
                //# address validation tokens (Section 8) to the client.
            }
        }
    }

    /// Called when the path is validated
    pub fn on_validated(&mut self) {
        self.challenge = None;
        self.state = State::Validated;

        // Enable the mtu controller to allow for PMTU discovery
        self.mtu_controller.enable()
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

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.2
    //# All QUIC
    //# packets that are not sent in a PMTU probe SHOULD be sized to fit
    //# within the maximum datagram size to avoid the datagram being
    //# fragmented or dropped ([RFC8085]).

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //# A PL MUST NOT send a datagram (other than a probe
    //# packet) with a size at the PL that is larger than the current
    //# PLPMTU.
    pub fn clamp_mtu(&self, requested_size: usize, transmission_mode: transmission::Mode) -> usize {
        let mtu = match transmission_mode {
            // Use the minimum MTU for loss recovery probes to allow detection of packets
            // lost when the previously confirmed path MTU is no longer supported.
            Mode::LossRecoveryProbing => MINIMUM_MTU as usize,
            // When MTU Probing, clamp to the size of the MTU we are attempting to validate
            Mode::MtuProbing => self.mtu_controller.probed_sized(),
            // Otherwise use the confirmed MTU
            Mode::Normal | Mode::PathValidation => self.mtu_controller.mtu(),
        };

        match self.state {
            State::Validated => requested_size.min(mtu),

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
            //# Prior to validating the client address, servers MUST NOT send more
            //# than three times as many bytes as the number of bytes they have
            //# received.
            State::AmplificationLimited { tx_bytes, rx_bytes } => {
                let limit = rx_bytes
                    .checked_mul(3)
                    .and_then(|v| v.checked_sub(tx_bytes))
                    .unwrap_or(0);
                requested_size.min(limit as usize).min(mtu)
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
    ///
    /// Note: This method is more conservative than strictly necessary in declaring a path at the
    ///       amplification limit. The path must be able to transmit at least a packet of the
    ///       `MINIMUM_MTU` bytes, otherwise the path is considered at the amplification limit.
    ///       TODO: Evaluate if this approach is too conservative
    pub fn at_amplification_limit(&self) -> bool {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.
        let mtu = MINIMUM_MTU as usize;
        self.clamp_mtu(mtu, transmission::Mode::Normal) < mtu
    }

    /// Returns the current PTO period
    pub fn pto_period(
        &self,
        space: s2n_quic_core::packet::number::PacketNumberSpace,
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
            State::Validated => {
                self.state = State::AmplificationLimited {
                    tx_bytes: 0,
                    rx_bytes: MINIMUM_MTU as _,
                };
            }
        }
    }
}

impl<CC: CongestionController> transmission::interest::Provider for Path<CC> {
    fn transmission_interest(&self) -> transmission::Interest {
        core::iter::empty()
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
            //# An endpoint MUST NOT delay transmission of a
            //# packet containing a PATH_RESPONSE frame unless constrained by
            //# congestion control.
            .chain(self.response_data.map(|_| transmission::Interest::NewData))
            .chain(
                self.challenge
                    .as_ref()
                    .map(|challenge| challenge.transmission_interest()),
            )
            .sum()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::{
        path::Path,
        recovery::congestion_controller::testing::unlimited::CongestionController as Unlimited,
    };
    use core::time::Duration;
    use s2n_quic_core::{connection, inet::SocketAddress, recovery::RttEstimator};

    pub fn helper_path() -> Path<Unlimited> {
        Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use super::*;
    use crate::{
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        path::{challenge::testing::helper_challenge, testing, Path},
    };
    use s2n_quic_core::{
        connection, endpoint,
        inet::SocketAddress,
        recovery::{CongestionController, CubicCongestionController, RttEstimator},
        time::{Clock, NoopClock},
        transmission,
    };

    #[test]
    fn response_data_should_only_be_sent_once() {
        // Setup:
        let mut path = testing::helper_path();
        let now = NoopClock {}.get_time();

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );

        // set response data
        let expected_data: [u8; 8] = [0; 8];
        path.on_path_challenge(&expected_data);
        assert_eq!(path.response_data.unwrap(), expected_data);

        // Trigger:
        path.on_transmit(&mut context); // send response data

        // Expectation:
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //= type=test
        //# An endpoint MUST NOT send more than one PATH_RESPONSE frame in
        //# response to one PATH_CHALLENGE frame; see Section 13.3.
        assert!(!path.response_data.is_some());

        assert_eq!(context.frame_buffer.len(), 1);
        let written_data = match context.frame_buffer.pop_front().unwrap().as_frame() {
            frame::Frame::PathResponse(frame) => Some(*frame.data),
            _ => None,
        };
        assert_eq!(written_data.unwrap(), expected_data);
    }

    #[test]
    fn on_timeout_should_set_challenge_to_none_on_challenge_abandonment() {
        // Setup:
        let mut path = testing::helper_path();
        let helper_challenge = helper_challenge();
        let expiration_time = helper_challenge.now + helper_challenge.abandon_duration;
        path = path.with_challenge(helper_challenge.challenge);

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper_challenge.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        path.on_transmit(&mut context); // send challenge and arm timer

        // Expectation:
        assert!(path.is_challenge_pending());
        assert!(path.challenge.is_some());

        // Trigger:
        path.on_timeout(expiration_time + Duration::from_millis(10));

        // Expectation:
        assert!(!path.is_challenge_pending());
        assert!(!path.challenge.is_some());
    }

    #[test]
    fn is_challenge_pending_should_return_false_if_challenge_is_not_set() {
        // Setup:
        let mut path = testing::helper_path();
        let helper_challenge = helper_challenge();

        // Expectation:
        assert!(!path.is_challenge_pending());
        assert!(!path.challenge.is_some());

        // Trigger:
        path = path.with_challenge(helper_challenge.challenge);

        // Expectation:
        assert!(path.is_challenge_pending());
        assert!(path.challenge.is_some());
    }

    #[test]
    fn on_path_challenge_should_set_reponse_data() {
        // Setup:
        let mut path = testing::helper_path();

        // Expectation:
        assert!(!path.response_data.is_some());

        // Trigger:
        let expected_data: [u8; 8] = [0; 8];
        path.on_path_challenge(&expected_data);

        // Expectation:
        assert!(path.response_data.is_some());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
    //= type=test
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
    //# echoing the data contained in the PATH_CHALLENGE frame in a
    //# PATH_RESPONSE frame.
    #[test]
    fn on_path_challenge_should_replace_reponse_data() {
        // Setup:
        let mut path = testing::helper_path();
        let expected_data: [u8; 8] = [0; 8];

        // Trigger 1:
        path.on_path_challenge(&expected_data);

        // Expectation 1:
        assert_eq!(path.response_data.unwrap(), expected_data);

        // Trigger 2:
        let new_expected_data: [u8; 8] = [1; 8];
        path.on_path_challenge(&new_expected_data);

        // Expectation 2:
        assert_ne!(expected_data, new_expected_data);
        assert_eq!(path.response_data.unwrap(), new_expected_data);
    }

    #[test]
    fn validate_path_response_should_only_validate_if_challenge_is_set() {
        // Setup:
        let mut path = testing::helper_path();
        let helper_challenge = helper_challenge();

        // Expectation:
        assert!(!path.is_validated());

        // Trigger:
        path = path.with_challenge(helper_challenge.challenge);
        path.on_path_response(&helper_challenge.expected_data);

        // Expectation:
        assert!(path.is_validated());
    }

    #[test]
    fn on_validated_should_change_state_to_validated_and_clear_challenge() {
        // Setup:
        let mut path = testing::helper_path();
        let helper_challenge = helper_challenge();
        path = path.with_challenge(helper_challenge.challenge);

        assert!(!path.is_validated());
        assert!(path.challenge.is_some());

        // Trigger:
        path.on_validated();

        // Expectation:
        assert!(path.is_validated());
        assert!(!path.challenge.is_some());
    }

    #[test]
    fn on_validated_when_already_validated_does_nothing() {
        // Setup:
        let mut path = testing::helper_path();
        path = path.with_challenge(helper_challenge().challenge);
        path.on_validated();

        // Trigger:
        path.on_validated();

        // Expectation:
        assert!(path.is_validated());
        assert!(path.challenge.is_none());
    }

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
        let mut path = testing::helper_path();

        // Verify we enforce the amplification limit if we can't send
        // at least 1 minimum sized packet
        let mut unblocked = path.on_bytes_received(1200);
        assert!(unblocked);
        path.on_bytes_transmitted((1200 * 2) + 1);
        assert!(path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::AmplificationLimited
        );

        unblocked = path.on_bytes_received(1200);
        assert!(!path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
        assert!(unblocked);

        path.on_bytes_transmitted((1200 * 6) + 1);
        assert!(path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::AmplificationLimited
        );
        unblocked = path.on_bytes_received(1200);
        assert!(!unblocked);

        path.on_validated();
        path.on_bytes_transmitted(24);
        // Validated paths should always be able to transmit
        assert!(!path.at_amplification_limit());
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
    fn amplification_limited_mtu_test() {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
        //= type=test
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.1
        //= type=test
        //# The server MUST also limit the number of bytes it sends before
        //# validating the address of the client; see Section 8.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.2
        //= type=test
        //# All QUIC
        //# packets that are not sent in a PMTU probe SHOULD be sized to fit
        //# within the maximum datagram size to avoid the datagram being
        //# fragmented or dropped ([RFC8085]).

        //= https://tools.ietf.org/rfc/rfc8899.txt#3
        //= type=test
        //# A PL MUST NOT send a datagram (other than a probe
        //# packet) with a size at the PL that is larger than the current
        //# PLPMTU.

        // TODO this would work better as a fuzz test

        for &transmission_mode in &[
            Mode::Normal,
            Mode::PathValidation,
            Mode::MtuProbing,
            Mode::LossRecoveryProbing,
        ] {
            let mut path = testing::helper_path();

            path.on_bytes_received(3);
            path.on_bytes_transmitted(8);

            // Verify we can transmit one more byte
            assert_eq!(path.clamp_mtu(1, transmission_mode), 1);
            assert_eq!(path.clamp_mtu(10, transmission_mode), 1);

            path.on_bytes_transmitted(1);
            // Verify we can't transmit any more bytes
            assert_eq!(path.clamp_mtu(1, transmission_mode), 0);
            assert_eq!(path.clamp_mtu(10, transmission_mode), 0);

            path.on_bytes_received(1);
            // Verify we can transmit up to 3 more bytes
            assert_eq!(path.clamp_mtu(1, transmission_mode), 1);
            assert_eq!(path.clamp_mtu(2, transmission_mode), 2);
            assert_eq!(path.clamp_mtu(4, transmission_mode), 3);

            path.on_validated();
            // Validated paths should always be able to transmit
            assert_eq!(path.clamp_mtu(4, transmission_mode), 4);
        }
    }

    #[test]
    fn clamp_mtu_test() {
        let mut path = testing::helper_path();
        path.on_validated();

        let mtu = 1472;
        let probed_size = 1500;

        path.mtu_controller = mtu::testing::test_controller(mtu, probed_size);

        assert_eq!(
            path.mtu_controller.mtu(),
            path.clamp_mtu(10000, transmission::Mode::Normal)
        );
        assert_eq!(
            path.mtu_controller.mtu(),
            path.clamp_mtu(10000, transmission::Mode::PathValidation)
        );
        assert_eq!(
            MINIMUM_MTU as usize,
            path.clamp_mtu(10000, transmission::Mode::LossRecoveryProbing)
        );
        assert_eq!(
            path.mtu_controller.probed_sized(),
            path.clamp_mtu(10000, transmission::Mode::MtuProbing)
        );
    }

    #[test]
    fn peer_validated_test() {
        let mut path = testing::helper_path();
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
            connection::LocalId::TEST_ID,
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
        path.congestion_controller.on_packets_lost(1, false, now);

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::RetransmissionOnly
        );

        // Lose remaining bytes
        path.congestion_controller.on_packets_lost(
            path.congestion_controller.congestion_window(),
            false,
            now,
        );

        // Since we are no longer congestion limited, there is no transmission constraint
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
    }
}
