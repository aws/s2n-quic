// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Path implementation

use crate::{
    connection,
    contexts::WriteContext,
    endpoint,
    endpoint::Type,
    recovery::{congestion_controller, CongestionController, RttEstimator},
    transmission::{self, Mode},
};
use s2n_quic_core::{
    counter::{Counter, Saturating},
    event::{self, IntoEvent},
    frame, packet, random,
    time::{timer, Timestamp},
};

mod challenge;
pub(crate) mod ecn;
mod manager;
pub(crate) mod mtu;

pub use challenge::*;
pub use manager::*;

/// re-export core
pub use s2n_quic_core::path::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// Path has no transmission limitations
    Validated,

    /// Path has not been validated and is subject to amplification limits
    AmplificationLimited {
        tx_allowance: Counter<u32, Saturating>,
    },
}

#[derive(Debug)]
pub struct Path<Config: endpoint::Config> {
    /// The peer's socket address
    pub handle: Config::PathHandle,
    /// The connection id of the peer
    pub peer_connection_id: connection::PeerId,
    /// The local connection id which the peer sends to
    pub local_connection_id: connection::LocalId,
    /// The path owns the roundtrip between peers
    pub rtt_estimator: RttEstimator,
    /// The congestion controller for the path
    pub congestion_controller: <Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController,
    /// Probe timeout backoff multiplier
    pub pto_backoff: u32,
    /// Tracks whether this path has passed Address or Path validation
    state: State,
    /// Controller for determining the maximum transmission unit of the path
    pub mtu_controller: mtu::Controller,
    /// Controller for determining the ECN capability of the path
    pub ecn_controller: ecn::Controller,

    /// True if the path has been validated by the peer
    peer_validated: bool,

    /// Challenge sent to the peer in a PATH_CHALLENGE
    challenge: Challenge,

    /// Received a Challenge and should echo back data in PATH_RESPONSE
    response_data: Option<challenge::Data>,

    /// True if the path is currently or at some point been an active path.
    ///
    /// A path becomes an active path if it receives a non-path-validation-probing
    /// packet. `activated` is a one way state used to mark paths that have been the
    /// active path at some point in the connection. This parameter is used to
    /// determine if the path should become the last_known_active_validated_path.
    activated: bool,

    /// True if the path is currently active
    is_active: bool,
}

impl<Config: endpoint::Config> Clone for Path<Config> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle,
            peer_connection_id: self.peer_connection_id,
            local_connection_id: self.local_connection_id,
            rtt_estimator: self.rtt_estimator,
            congestion_controller: self.congestion_controller.clone(),
            pto_backoff: self.pto_backoff,
            state: self.state,
            mtu_controller: self.mtu_controller.clone(),
            ecn_controller: self.ecn_controller.clone(),
            peer_validated: self.peer_validated,
            challenge: self.challenge.clone(),
            response_data: self.response_data,
            activated: self.activated,
            is_active: self.is_active,
        }
    }
}

/// A Path holds the local and peer socket addresses, connection ids, and state. It can be
/// validated or pending validation.
impl<Config: endpoint::Config> Path<Config> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handle: Config::PathHandle,
        peer_connection_id: connection::PeerId,
        local_connection_id: connection::LocalId,
        rtt_estimator: RttEstimator,
        congestion_controller: <Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController,
        peer_validated: bool,
        max_mtu: MaxMtu,
    ) -> Path<Config> {
        let state = match Config::ENDPOINT_TYPE {
            Type::Server => {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
                //# If the client IP address has changed, the
                //# server MUST adhere to the anti-amplification limit; see Section 8.
                // Start each path in State::AmplificationLimited until it has been validated.
                State::AmplificationLimited {
                    tx_allowance: Default::default(),
                }
            }
            //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
            //# Clients are only constrained by the congestion controller.
            Type::Client => State::Validated,
        };
        let peer_socket_address = handle.remote_address();
        Path {
            handle,
            peer_connection_id,
            local_connection_id,
            rtt_estimator,
            congestion_controller,
            pto_backoff: INITIAL_PTO_BACKOFF,
            state,
            mtu_controller: mtu::Controller::new(max_mtu, &peer_socket_address),
            ecn_controller: ecn::Controller::default(),
            peer_validated,
            challenge: Challenge::disabled(),
            response_data: None,
            activated: false,
            is_active: false,
        }
    }

    #[inline]
    pub fn remote_address(&self) -> RemoteAddress {
        self.handle.remote_address()
    }

    #[inline]
    pub fn local_address(&self) -> LocalAddress {
        self.handle.local_address()
    }

    #[inline]
    pub fn set_challenge(&mut self, challenge: Challenge) {
        self.challenge = challenge;
    }

    #[inline]
    pub fn abandon_challenge<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
        path_id: u64,
    ) {
        self.challenge
            .abandon(publisher, path_event!(self, path_id));
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Called when bytes have been transmitted on this path
    #[inline]
    pub fn on_bytes_transmitted(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }

        debug_assert_ne!(
            self.clamp_mtu(bytes, transmission::Mode::Normal),
            0,
            "path should not transmit when amplification limited; tried to transmit {}",
            bytes
        );

        if let State::AmplificationLimited { tx_allowance, .. } = &mut self.state {
            *tx_allowance -= bytes as u32
        }
    }

    /// Called when bytes have been received on this path
    /// Returns true if receiving these bytes unblocked the
    /// path from being amplification limited
    #[inline]
    pub fn on_bytes_received(&mut self, bytes: usize) -> bool {
        let was_at_amplification_limit = self.at_amplification_limit();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
        //# For the purposes of
        //# avoiding amplification prior to address validation, servers MUST
        //# count all of the payload bytes received in datagrams that are
        //# uniquely attributed to a single connection.
        //
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.
        //
        if let State::AmplificationLimited { tx_allowance } = &mut self.state {
            *tx_allowance += bytes.saturating_mul(3) as u32;
        }

        was_at_amplification_limit && !self.at_amplification_limit()
    }

    #[inline]
    pub fn on_timeout<Rnd: random::Generator, Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        path_id: Id,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) {
        self.challenge
            .on_timeout(timestamp, publisher, path_event!(self, path_id));
        self.mtu_controller.on_timeout(timestamp);
        self.ecn_controller.on_timeout(
            timestamp,
            path_event!(self, path_id),
            random_generator,
            self.rtt_estimator.smoothed_rtt(),
            publisher,
        );
    }

    /// Returns true if this path is able to transmit packets at the given timestamp
    #[inline]
    pub fn can_transmit(&self, timestamp: Timestamp) -> bool {
        !self.at_amplification_limit()
            && self
                .congestion_controller
                .earliest_departure_time()
                .map_or(true, |edt| edt.has_elapsed(timestamp))
    }

    /// Only PATH_CHALLENGE and PATH_RESPONSE frames should be transmitted here.
    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE frame was received.
        if let Some(response_data) = &mut self.response_data {
            let frame = frame::PathResponse {
                data: response_data,
            };
            if context.write_frame(&frame).is_some() {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
                //# An endpoint MUST NOT send more than one PATH_RESPONSE frame in
                //# response to one PATH_CHALLENGE frame; see Section 13.3.
                self.response_data = None;
            }
        }

        self.challenge.on_transmit(context)
    }

    /// Check if path validation was attempted and failed.
    #[inline]
    pub fn failed_validation(&self) -> bool {
        // PATH_CHALLENGE is not used for validating the initial path and is disabled. Check if
        // the challenge is disabled before executing the following block since there won't be
        // a last_known_validated_path.
        !self.challenge.is_disabled() && !self.is_validated() && !self.is_challenge_pending()
    }

    #[inline]
    pub fn is_challenge_pending(&self) -> bool {
        self.challenge.is_pending()
    }

    #[inline]
    pub fn is_response_pending(&self) -> bool {
        self.response_data.is_some()
    }

    #[inline]
    pub fn on_path_challenge(&mut self, response: &challenge::Data) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
        //# echoing the data contained in the PATH_CHALLENGE frame in a
        //# PATH_RESPONSE frame.
        self.response_data = Some(*response);
    }

    /// Validates the path if the PATH_RESPONSE data matches the PATH_CHALLENGE data
    /// and returns if the path was validated.
    #[inline]
    pub fn on_path_response(&mut self, response: &[u8]) -> bool {
        if self.challenge.on_validated(response) {
            self.on_validated();

            return true;

            //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
            //= type=TODO
            //# After verifying a new client address, the server SHOULD send new
            //# address validation tokens (Section 8) to the client.
        }

        false
    }

    /// Called when a handshake packet is received.
    ///
    /// Receiving a handshake packet acts as path validation for the initial path
    #[inline]
    pub fn on_handshake_packet(&mut self) {
        self.on_validated();
    }

    /// Checks if the peer has started using a different destination Connection Id.
    ///
    /// The CleartextShort packet guarantees the packet has been validated
    /// (authenticated and de-duped).
    pub fn on_process_local_connection_id<Pub: event::ConnectionPublisher>(
        &mut self,
        path_id: Id,
        packet: &packet::short::CleartextShort<'_>,
        local_connection_id: &connection::LocalId,
        publisher: &mut Pub,
    ) {
        debug_assert_eq!(
            packet.destination_connection_id(),
            local_connection_id.as_ref()
        );

        if &self.local_connection_id != local_connection_id {
            publisher.on_connection_id_updated(event::builder::ConnectionIdUpdated {
                path_id: path_id.into_event(),
                cid_consumer: endpoint::Location::Remote,
                previous: self.local_connection_id.into_event(),
                current: local_connection_id.into_event(),
            });
            self.local_connection_id = *local_connection_id;
        }
    }

    /// Called when the path is validated
    #[inline]
    fn on_validated(&mut self) {
        self.state = State::Validated;

        if self.is_peer_validated() {
            self.on_fully_validated();
        }
    }

    /// Returns whether this path has passed address validation
    #[inline]
    pub fn is_validated(&self) -> bool {
        self.state == State::Validated
    }

    /// The path received a non-path-validation-probing packet so mark it as activated.
    #[inline]
    pub fn on_activated(&mut self) {
        self.activated = true;
    }

    /// Returns if the path is currently or at some point been an active path.
    #[inline]
    pub fn is_activated(&self) -> bool {
        self.activated
    }

    /// Marks the path as peer validated
    #[inline]
    pub fn on_peer_validated(&mut self) {
        self.peer_validated = true;

        if self.is_validated() {
            self.on_fully_validated();
        }
    }

    /// Returns whether this path has been validated by the peer
    #[inline]
    pub fn is_peer_validated(&self) -> bool {
        self.peer_validated
    }

    /// Called when the path has been validated locally, and also by the peer
    fn on_fully_validated(&mut self) {
        // Enable the mtu controller to allow for PMTU discovery
        self.mtu_controller.enable()
    }

    #[inline]
    fn mtu(&self, transmission_mode: transmission::Mode) -> usize {
        match transmission_mode {
            // Use the minimum MTU for loss recovery probes to allow detection of packets
            // lost when the previously confirmed path MTU is no longer supported.
            //
            // The priority during PathValidationOnly is to validate the path, so the
            // minimum MTU is used to avoid packet loss due to MTU limits.
            Mode::LossRecoveryProbing | Mode::PathValidationOnly => MINIMUM_MTU as usize,
            // When MTU Probing, clamp to the size of the MTU we are attempting to validate
            Mode::MtuProbing => self.mtu_controller.probed_sized(),
            // Otherwise use the confirmed MTU
            Mode::Normal => self.mtu_controller.mtu(),
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
    //# The server MUST also limit the number of bytes it sends before
    //# validating the address of the client; see Section 8.

    //= https://www.rfc-editor.org/rfc/rfc9000#section-14.2
    //# All QUIC
    //# packets that are not sent in a PMTU probe SHOULD be sized to fit
    //# within the maximum datagram size to avoid the datagram being
    //# fragmented or dropped [RFC8085].

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# A PL MUST NOT send a datagram (other than a probe
    //# packet) with a size at the PL that is larger than the current
    //# PLPMTU.
    #[inline]
    pub fn clamp_mtu(&self, requested_size: usize, transmission_mode: transmission::Mode) -> usize {
        let mtu = self.mtu(transmission_mode);

        match self.state {
            State::Validated => requested_size.min(mtu),

            // https://github.com/aws/s2n-quic/issues/695
            // Note: while a 3X check if performed, the `limit` value is not used
            // to restrict the MTU. There are two reasons for this:
            // - Expanding to the full MTU allows for MTU validation during connection migration.
            // - Networking infrastructure cares more about number of packets than bytes for
            // anti-amplification.
            State::AmplificationLimited { tx_allowance } => {
                if tx_allowance > 0 {
                    requested_size.min(mtu)
                } else {
                    0
                }
            }
        }
    }

    #[inline]
    pub fn transmission_constraint(&self) -> transmission::Constraint {
        if self.at_amplification_limit() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
            //# Prior to validating the client address, servers MUST NOT send more
            //# than three times as many bytes as the number of bytes they have
            //# received.
            transmission::Constraint::AmplificationLimited
        } else if self.congestion_controller.is_congestion_limited() {
            if self.congestion_controller.requires_fast_retransmission() {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                //# If the congestion window is reduced immediately, a
                //# single packet can be sent prior to reduction.  This speeds up loss
                //# recovery if the data in the lost packet is retransmitted and is
                //# similar to TCP as described in Section 5 of [RFC6675].
                transmission::Constraint::RetransmissionOnly
            } else {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7
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
    #[inline]
    pub fn at_amplification_limit(&self) -> bool {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.
        let mtu = MINIMUM_MTU as usize;
        self.clamp_mtu(mtu, transmission::Mode::Normal) < mtu
    }

    /// Returns the current PTO period
    #[inline]
    pub fn pto_period(
        &self,
        space: s2n_quic_core::packet::number::PacketNumberSpace,
    ) -> core::time::Duration {
        self.rtt_estimator.pto_period(self.pto_backoff, space)
    }

    /// Resets the PTO backoff to the initial value
    #[inline]
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
                    tx_allowance: Counter::new(MINIMUM_MTU as u32 * 3),
                };
            }
        }
    }

    #[inline]
    pub fn max_mtu(&self) -> MaxMtu {
        self.mtu_controller.max_mtu()
    }

    // Compare a Path based on its PathHandle.
    //
    // Currently the local_address on the Client connection is unknown and set to
    // a default un-specified value; therefore only the remote_address is used
    // to compare Paths.
    fn eq_by_handle(&self, handle: &Config::PathHandle) -> bool {
        if Config::ENDPOINT_TYPE.is_client() {
            // TODO: https://github.com/aws/s2n-quic/issues/954
            // Possibly research a strategy to populate the local_address for Client endpoint
            s2n_quic_core::path::Handle::eq(&self.handle.remote_address(), &handle.remote_address())
        } else {
            self.handle.eq(handle)
        }
    }
}

impl<Config: endpoint::Config> timer::Provider for Path<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.challenge.timers(query)?;
        self.mtu_controller.timers(query)?;
        self.ecn_controller.timers(query)?;

        Ok(())
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for Path<Config> {
    /// Indicate if the path is interested in transmitting PATH_CHALLENGE or
    /// PATH_RESPONSE frames.
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //# An endpoint MUST NOT delay transmission of a
        //# packet containing a PATH_RESPONSE frame unless constrained by
        //# congestion control.
        if self.response_data.is_some() {
            query.on_new_data()?;
        }

        self.challenge.transmission_interest(query)?;

        Ok(())
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::{
        endpoint,
        path::{Path, DEFAULT_MAX_MTU},
    };
    use core::time::Duration;
    use s2n_quic_core::{connection, recovery::RttEstimator};

    pub fn helper_path_server() -> Path<endpoint::testing::Server> {
        Path::new(
            Default::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            true,
            DEFAULT_MAX_MTU,
        )
    }

    pub fn helper_path_client() -> Path<endpoint::testing::Client> {
        Path::new(
            Default::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        endpoint::testing::Server as Config,
        path,
        path::{challenge::testing::helper_challenge, testing, testing::helper_path_client},
    };
    use core::time::Duration;
    use s2n_quic_core::{
        connection, endpoint,
        event::testing::Publisher,
        recovery::{CongestionController, RttEstimator},
        time::{Clock, NoopClock},
        transmission,
    };

    type Path = super::Path<Config>;

    #[test]
    fn response_data_should_only_be_sent_once() {
        // Setup:
        let mut path = testing::helper_path_server();
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
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //= type=test
        //# An endpoint MUST NOT send more than one PATH_RESPONSE frame in
        //# response to one PATH_CHALLENGE frame; see Section 13.3.
        assert!(path.response_data.is_none());

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
        let mut publisher = Publisher::snapshot();
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();
        let expiration_time = helper_challenge.now + helper_challenge.abandon_duration;
        path.set_challenge(helper_challenge.challenge);

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
        assert!(path.challenge.is_pending());

        // Trigger:
        path.on_timeout(
            expiration_time + Duration::from_millis(10),
            path::Id::test_id(),
            &mut random::testing::Generator(123),
            &mut publisher,
        );

        // Expectation:
        assert!(!path.is_challenge_pending());
        assert!(!path.challenge.is_pending());
    }

    #[test]
    fn is_challenge_pending_should_return_false_if_challenge_is_not_set() {
        // Setup:
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();

        // Expectation:
        assert!(!path.is_challenge_pending());
        assert!(!path.challenge.is_pending());

        // Trigger:
        path.set_challenge(helper_challenge.challenge);

        // Expectation:
        assert!(path.is_challenge_pending());
        assert!(path.challenge.is_pending());
    }

    #[test]
    fn first_path_in_disabled_state_cant_fail_validation() {
        // Setup:
        let path = testing::helper_path_server();

        // Expectation:
        assert!(path.challenge.is_disabled());
        assert!(!path.is_challenge_pending());
        assert!(!path.is_validated());

        assert!(!path.failed_validation());
    }

    #[test]
    fn failed_validation() {
        // Setup:
        let mut publisher = Publisher::snapshot();
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();

        path.set_challenge(helper_challenge.challenge);
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper_challenge.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        path.on_transmit(&mut context); // send challenge and arm timer

        let expiration_time = helper_challenge.now + helper_challenge.abandon_duration;

        // Trigger:
        path.on_timeout(
            expiration_time + Duration::from_millis(10),
            path::Id::test_id(),
            &mut random::testing::Generator(123),
            &mut publisher,
        );

        // Expectation:
        assert!(!path.challenge.is_disabled());
        assert!(!path.is_challenge_pending());
        assert!(!path.is_validated());

        assert!(path.failed_validation());
    }

    #[test]
    fn abandon_challenge() {
        // Setup:
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();
        path.set_challenge(helper_challenge.challenge);
        let mut publisher = event::testing::Publisher::snapshot();

        // Trigger:
        path.abandon_challenge(&mut publisher, 0);

        // Expectation:
        assert!(!path.challenge.is_pending());
    }

    #[test]
    fn on_path_challenge_should_set_response_data() {
        // Setup:
        let mut path = testing::helper_path_server();

        // Expectation:
        assert!(path.response_data.is_none());

        // Trigger:
        let expected_data: [u8; 8] = [0; 8];
        path.on_path_challenge(&expected_data);

        // Expectation:
        assert!(path.response_data.is_some());
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
    //= type=test
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
    //# echoing the data contained in the PATH_CHALLENGE frame in a
    //# PATH_RESPONSE frame.
    #[test]
    fn on_path_challenge_should_replace_response_data() {
        // Setup:
        let mut path = testing::helper_path_server();
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
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();

        // Expectation:
        assert!(!path.is_validated());

        // Trigger:
        path.set_challenge(helper_challenge.challenge);
        assert!(path.on_path_response(&helper_challenge.expected_data));

        // Expectation:
        assert!(path.is_validated());
    }

    #[test]
    fn on_validated_should_change_state_to_validated_and_clear_challenge() {
        // Setup:
        let mut path = testing::helper_path_server();
        let helper_challenge = helper_challenge();
        path.set_challenge(helper_challenge.challenge);

        assert!(!path.is_validated());
        assert!(path.challenge.is_pending());

        // Trigger:
        path.on_validated();

        // Expectation:
        assert!(path.is_validated());
        assert!(path.challenge.is_pending());
    }

    #[test]
    fn on_validated_when_already_validated_does_nothing() {
        // Setup:
        let mut path = testing::helper_path_server();
        path.set_challenge(helper_challenge().challenge);
        path.on_validated();

        // Trigger:
        path.on_validated();

        // Expectation:
        assert!(path.is_validated());
        assert!(path.challenge.is_pending());
    }

    #[test]
    fn amplification_limit_test() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# If the client IP address has changed, the server MUST
        //# adhere to the anti-amplification limit; see Section 8.
        // This is tested here by verifying a new Path starts in State::AmplificationLimited

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
        //= type=test
        //# For the purposes of
        //# avoiding amplification prior to address validation, servers MUST
        //# count all of the payload bytes received in datagrams that are
        //# uniquely attributed to a single connection.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# If the client IP address has changed, the server MUST
        //# adhere to the anti-amplification limit; see Section 8.
        // This tests the IP change because a new path is created when a new peer_address is
        // detected. This new path should always start in State::Pending.
        let mut path = testing::helper_path_server();

        // Verify we enforce the amplification limit if we can't send
        // at least 1 minimum sized packet
        let mut unblocked = path.on_bytes_received(1200);
        assert!(unblocked);
        path.on_bytes_transmitted((1200 * 2) + 1);

        // we round up to the nearest mtu
        assert!(!path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );

        unblocked = path.on_bytes_received(1200);
        assert!(!path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
        // If we were not amplification limited previously, receiving
        // more bytes doesn't unblock
        assert!(!unblocked);

        path.on_bytes_transmitted((1200 * 6) + 1);
        assert!(path.at_amplification_limit());
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::AmplificationLimited
        );
        unblocked = path.on_bytes_received(1200);
        assert!(unblocked);

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

        // Clients are not amplification limited
        let path = helper_path_client();
        assert!(path.is_validated());
    }

    #[test]
    fn amplification_limited_mtu_test() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
        //= type=test
        //# Prior to validating the client address, servers MUST NOT send more
        //# than three times as many bytes as the number of bytes they have
        //# received.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
        //= type=test
        //# The server MUST also limit the number of bytes it sends before
        //# validating the address of the client; see Section 8.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.2
        //= type=test
        //# All QUIC
        //# packets that are not sent in a PMTU probe SHOULD be sized to fit
        //# within the maximum datagram size to avoid the datagram being
        //# fragmented or dropped [RFC8085].

        //= https://www.rfc-editor.org/rfc/rfc8899#section-3
        //= type=test
        //# A PL MUST NOT send a datagram (other than a probe
        //# packet) with a size at the PL that is larger than the current
        //# PLPMTU.

        // TODO this would work better as a fuzz test

        for &transmission_mode in &[
            Mode::Normal,
            Mode::PathValidationOnly,
            Mode::MtuProbing,
            Mode::LossRecoveryProbing,
        ] {
            let mut path = testing::helper_path_server();
            // Verify we can transmit up to the mtu
            let mtu = path.mtu(transmission_mode);

            path.on_bytes_received(3);
            path.on_bytes_transmitted(8);

            assert_eq!(path.clamp_mtu(1, transmission_mode), 1);
            assert_eq!(path.clamp_mtu(10, transmission_mode), 10);
            assert_eq!(path.clamp_mtu(1800, transmission_mode), mtu);

            path.on_bytes_transmitted(1);
            // Verify we can't transmit any more bytes
            assert_eq!(path.clamp_mtu(1, transmission_mode), 0);
            assert_eq!(path.clamp_mtu(10, transmission_mode), 0);

            path.on_bytes_received(1);
            // Verify we can transmit up to 3 more bytes
            assert_eq!(path.clamp_mtu(1, transmission_mode), 1);
            assert_eq!(path.clamp_mtu(10, transmission_mode), 10);
            assert_eq!(path.clamp_mtu(1800, transmission_mode), mtu);

            path.on_validated();
            // Validated paths should always be able to transmit
            assert_eq!(path.clamp_mtu(4, transmission_mode), 4);
        }
    }

    #[test]
    fn clamp_mtu_for_validated_path() {
        let mut path = testing::helper_path_server();
        path.on_validated();
        let mtu = 1472;
        let probed_size = 1500;
        path.mtu_controller = mtu::testing::test_controller(mtu, probed_size);

        assert_eq!(
            path.mtu_controller.mtu(),
            path.clamp_mtu(10000, transmission::Mode::Normal)
        );
        assert_eq!(
            MINIMUM_MTU as usize,
            path.clamp_mtu(10000, transmission::Mode::PathValidationOnly)
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
    fn path_mtu() {
        let mut path = testing::helper_path_server();
        path.on_bytes_received(1);
        let mtu = 1472;
        let probed_size = 1500;
        path.mtu_controller = mtu::testing::test_controller(mtu, probed_size);

        assert_eq!(
            path.mtu_controller.mtu(),
            path.mtu(transmission::Mode::Normal)
        );
        assert_eq!(
            MINIMUM_MTU as usize,
            path.mtu(transmission::Mode::PathValidationOnly)
        );
        assert_eq!(
            MINIMUM_MTU as usize,
            path.mtu(transmission::Mode::LossRecoveryProbing)
        );
        assert_eq!(
            path.mtu_controller.probed_sized(),
            path.mtu(transmission::Mode::MtuProbing)
        );
    }

    #[test]
    fn clamp_mtu_when_tx_more_than_rx() {
        let mut path = testing::helper_path_server();
        let mtu = 1472;
        let probed_size = 1500;
        path.mtu_controller = mtu::testing::test_controller(mtu, probed_size);

        assert_eq!(0, path.clamp_mtu(10000, transmission::Mode::Normal));

        path.on_bytes_received(1);
        assert_eq!(
            path.mtu_controller.mtu(),
            path.clamp_mtu(10000, transmission::Mode::Normal)
        );

        path.on_bytes_transmitted(100);
        assert_eq!(0, path.clamp_mtu(10000, transmission::Mode::Normal));
    }

    #[test]
    fn peer_validated_test() {
        let mut path = testing::helper_path_client();

        assert!(!path.is_peer_validated());

        path.on_peer_validated();

        assert!(path.is_peer_validated());
    }

    #[test]
    fn transmission_constraint_test() {
        let mut path = Path::new(
            Default::default(),
            connection::PeerId::try_from_bytes(&[]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        let now = NoopClock.get_time();
        let random = &mut random::testing::Generator::default();
        path.on_validated();

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );

        // Fill up the congestion controller
        let packet_info = path.congestion_controller.on_packet_sent(
            now,
            path.congestion_controller.congestion_window() as usize,
            &path.rtt_estimator,
        );

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::CongestionLimited
        );

        // Lose a byte to enter recovery
        path.congestion_controller
            .on_packet_lost(1, packet_info, false, false, random, now);
        path.congestion_controller.requires_fast_retransmission = true;

        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::RetransmissionOnly
        );

        // Lose remaining bytes
        path.congestion_controller.on_packet_lost(
            path.congestion_controller.congestion_window(),
            packet_info,
            false,
            false,
            random,
            now,
        );
        path.congestion_controller.requires_fast_retransmission = false;

        // Since we are no longer congestion limited, there is no transmission constraint
        assert_eq!(
            path.transmission_constraint(),
            transmission::Constraint::None
        );
    }

    #[test]
    fn on_closing_validated_path() {
        let mut path = testing::helper_path_server();
        path.on_validated();
        assert!(path.is_validated());

        // Trigger:
        path.on_closing();

        // Expectation:
        match path.state {
            path::State::Validated => panic!("transition to AmplificationLimited when closing"),
            path::State::AmplificationLimited { tx_allowance } => {
                assert_eq!(*tx_allowance, (MINIMUM_MTU * 3) as u32)
            }
        }
    }

    // Maintain amplification limits if already in AmplificationLimited state
    #[test]
    fn on_closing_not_validated_path() {
        let mut path = testing::helper_path_server();
        assert!(!path.is_validated());

        // Trigger:
        path.on_closing();

        // Expectation:
        match path.state {
            path::State::Validated => panic!("transition to AmplificationLimited when closing"),
            path::State::AmplificationLimited { tx_allowance } => {
                assert_eq!(*tx_allowance, 0)
            }
        }
    }
}
