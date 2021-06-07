// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Manager implementation

use crate::{
    connection::PeerIdRegistry,
    path::{challenge, Path},
    transmission,
};
use s2n_quic_core::{
    ack, connection, frame,
    inet::{DatagramInfo, SocketAddress},
    packet::number::PacketNumberSpace,
    random,
    recovery::{congestion_controller, RttEstimator},
    stateless_reset,
    time::Timestamp,
    transport,
};
use smallvec::SmallVec;

/// The amount of Paths that can be maintained without using the heap
const INLINE_PATH_LEN: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Debug)]
pub struct Manager<CCE: congestion_controller::Endpoint> {
    /// Path array
    paths: SmallVec<[Path<CCE::CongestionController>; INLINE_PATH_LEN]>,

    /// Registry of `connection::PeerId`s
    peer_id_registry: PeerIdRegistry,

    /// Index to the active path
    active: u8,

    /// Index of last known validated path
    last_known_validated_path: Option<u8>,
}

impl<CCE: congestion_controller::Endpoint> Manager<CCE> {
    pub fn new(
        initial_path: Path<CCE::CongestionController>,
        peer_id_registry: PeerIdRegistry,
    ) -> Self {
        Manager {
            paths: SmallVec::from_elem(initial_path, 1),
            peer_id_registry,
            active: 0,
            last_known_validated_path: None,
        }
    }

    /// Update the active path
    #[allow(dead_code)]
    fn update_active_path(&mut self, path_id: Id) -> Result<(), transport::Error> {
        debug_assert!(path_id != Id(self.active));

        if self.active_path().is_validated() {
            self.last_known_validated_path = Some(self.active);
        }

        let new_path_idx = path_id.0;
        // Attempt to consume a new connection id in case it has been retired since the last use.
        let peer_connection_id = self.paths[new_path_idx as usize].peer_connection_id;

        // The path's connection id might have retired since we last used it. Check if it is still
        // active, otherwise try and consume a new connection id.
        let use_peer_connection_id = if self.peer_id_registry.is_active(&peer_connection_id) {
            peer_connection_id
        } else {
            // TODO https://github.com/awslabs/s2n-quic/issues/669
            // If there are no new connection ids the peer is responsible for
            // providing additional connection ids to continue.
            //
            // Insufficient connection ids should not cause the connection to close.
            // Investigate api after this is used.
            self.peer_id_registry
                .consume_new_id()
                .ok_or(transport::Error::INTERNAL_ERROR)?
        };

        self[path_id].peer_connection_id = use_peer_connection_id;

        self.active = new_path_idx;
        Ok(())
    }

    /// Return the active path
    pub fn active_path(&self) -> &Path<CCE::CongestionController> {
        &self.paths[self.active as usize]
    }

    /// Return a mutable reference to the active path
    pub fn active_path_mut(&mut self) -> &mut Path<CCE::CongestionController> {
        &mut self.paths[self.active as usize]
    }

    /// Return the Id of the active path
    pub fn active_path_id(&self) -> Id {
        Id(self.active)
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path(
        &self,
        peer_address: &SocketAddress,
    ) -> Option<(Id, &Path<CCE::CongestionController>)> {
        self.paths
            .iter()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id as u8), path))
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path_mut(
        &mut self,
        peer_address: &SocketAddress,
    ) -> Option<(Id, &mut Path<CCE::CongestionController>)> {
        self.paths
            .iter_mut()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id as u8), path))
    }

    /// Called when a datagram is received on a connection
    /// Upon success, returns a `(Id, bool)` containing the path ID and a boolean that is
    /// true if the path had been amplification limited prior to receiving the datagram
    /// and is now no longer amplification limited.
    #[allow(unused_variables)]
    pub fn on_datagram_received<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        limits: &connection::Limits,
        handshake_confirmed: bool,
        congestion_controller_endpoint: &mut CCE,
        random_generator: &mut Rnd,
    ) -> Result<(Id, bool), transport::Error> {
        if let Some((id, path)) = self.path_mut(&datagram.remote_address) {
            let unblocked = path.on_bytes_received(datagram.payload_len);
            return Ok((id, unblocked));
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.5
        //= type=TODO
        //= tracking-issue=316
        //# Similarly, an endpoint MUST NOT reuse a connection ID when sending to
        //# more than one destination address.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.5
        //= type=TODO
        //= tracking-issue=316
        //# Due to network changes outside
        //# the control of its peer, an endpoint might receive packets from a new
        //# source address with the same destination connection ID, in which case
        //# it MAY continue to use the current connection ID with the new remote
        //# address while still sending from the same local address.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# The design of QUIC relies on endpoints retaining a stable address
        //# for the duration of the handshake.  An endpoint MUST NOT initiate
        //# connection migration before the handshake is confirmed, as defined
        //# in section 4.1.2 of [QUIC-TLS].
        if !handshake_confirmed {
            return Err(transport::Error::PROTOCOL_VIOLATION);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# If the peer
        //# violates this requirement, the endpoint MUST either drop the incoming
        //# packets on that path without generating a stateless reset or proceed
        //# with path validation and allow the peer to migrate.  Generating a
        //# stateless reset or closing the connection would allow third parties
        //# in the network to cause connections to close by spoofing or otherwise
        //# manipulating observed traffic.

        // TODO set alpn if available

        self.handle_connection_migration(datagram, congestion_controller_endpoint, random_generator)
    }

    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    fn handle_connection_migration<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut CCE,
        random_generator: &mut Rnd,
    ) -> Result<(Id, bool), transport::Error> {
        // Since we are not currently supporting connection migration (whether it was deliberate or
        // not), we will error our at this point to avoid re-using a peer connection ID.
        // TODO: This would be better handled as a stateless reset so the peer can terminate the
        //       connection immediately. https://github.com/awslabs/s2n-quic/issues/317
        // We only enable connection migration for testing
        #[cfg(not(any(feature = "testing", test)))]
        return Err(transport::Error::INTERNAL_ERROR);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
        //= type=TODO
        //# Because port-only changes are commonly the
        //# result of NAT rebinding or other middlebox activity, the endpoint MAY
        //# instead retain its congestion control state and round-trip estimate
        //# in those cases instead of reverting to initial values.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
        //# Note that since the endpoint will not have any round-trip
        //# time measurements to this address, the estimate SHOULD be the default
        //# initial value; see [QUIC-RECOVERY].
        let rtt = RttEstimator::new(self.active_path().rtt_estimator.max_ack_delay());
        let path_info = congestion_controller::PathInfo::new(&datagram.remote_address);
        let cc = congestion_controller_endpoint.new_congestion_controller(path_info);

        let peer_connection_id = {
            if self.active_path().local_connection_id != datagram.destination_connection_id {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.5
                //# Similarly, an endpoint MUST NOT reuse a connection ID when sending to
                //# more than one destination address.

                // Peer has intentionally tried to migrate to this new path because they changed
                // their destination_connection_id, so we will change our destination_connection_id as well.
                self.peer_id_registry
                    .consume_new_id()
                    // TODO https://github.com/awslabs/s2n-quic/issues/669
                    // Insufficient connection ids should not cause the connection to close.
                    // Investigate if there is a safer way to expose an error here.
                    //
                    // Currently all errors are ignored when calling on_datagram_received in endpoint/mod.rs
                    .ok_or(transport::Error::INTERNAL_ERROR)?
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.5
                //# Due to network changes outside
                //# the control of its peer, an endpoint might receive packets from a new
                //# source address with the same destination connection ID, in which case
                //# it MAY continue to use the current connection ID with the new remote
                //# address while still sending from the same local address.
                self.active_path().peer_connection_id
            }
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
        //# frame so that it can associate the peer's response with the
        //# corresponding PATH_CHALLENGE.
        let mut data: challenge::Data = [0; 8];
        random_generator.public_random_fill(&mut data);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
        //# Until a peer's address is deemed valid, an endpoint MUST
        //# limit the rate at which it sends data to this address.
        let mut path = Path::new(
            datagram.remote_address,
            peer_connection_id,
            datagram.destination_connection_id,
            rtt,
            cc,
            true,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#8.2.4
        //# Endpoints SHOULD abandon path validation based on a timer.  When
        //# setting this timer, implementations are cautioned that the new path
        //# could have a longer round-trip time than the original. A value of
        //# three times the larger of the current Probe Timeout (PTO) or the PTO
        //# for the new path (that is, using kInitialRtt as defined in
        //# [QUIC-RECOVERY]) is RECOMMENDED.
        let abandon_duration = path.pto_period(PacketNumberSpace::ApplicationData);
        let abandon_duration = 3 * abandon_duration.max(
            self.active_path()
                .pto_period(PacketNumberSpace::ApplicationData),
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.6.3
        //# Servers SHOULD initiate path validation to the client's new address
        //# upon receiving a probe packet from a different address.
        let challenge = challenge::Challenge::new(datagram.timestamp + abandon_duration, data);
        path = path.with_challenge(challenge);

        let unblocked = path.on_bytes_received(datagram.payload_len);
        // create a new path
        let id = Id(self.paths.len() as u8);
        self.paths.push(path);

        Ok((id, unblocked))
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> + '_ {
        self.paths.iter().flat_map(|p| p.timers())
    }

    /// Writes any frames the path manager wishes to transmit to the given context
    pub fn on_transmit<W: transmission::WriteContext>(&mut self, context: &mut W) {
        self.peer_id_registry.on_transmit(context)

        // TODO Add in per-path constraints based on whether a Challenge needs to be
        // transmitted.
    }

    /// Iterate paths pending a path verification
    pub fn pending_paths(&mut self, timestamp: Timestamp) -> PendingPaths<CCE> {
        PendingPaths::new(self, timestamp)
    }

    /// Called when packets are acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.peer_id_registry.on_packet_ack(ack_set)
    }

    /// Called when packets are lost
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.peer_id_registry.on_packet_loss(ack_set)
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
    //= type=TODO
    //= tracking-issue=404
    //= feature=Client connection migration
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
    //# echoing the data contained in the PATH_CHALLENGE frame in a
    //# PATH_RESPONSE frame.
    pub fn on_path_challenge(
        &mut self,
        _peer_address: &SocketAddress,
        _challenge: frame::path_challenge::PathChallenge,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //= type=TODO
        //= tracking-issue=406
        //= feature=Connection migration
        //# An endpoint MUST NOT delay transmission of a
        //# packet containing a PATH_RESPONSE frame unless constrained by
        //# congestion control.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //= type=TODO
        //= tracking-issue=407
        //= feature=Connection migration
        //# An endpoint MUST NOT send more than one PATH_RESPONSE frame in
        //# response to one PATH_CHALLENGE frame; see Section 13.3.
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.3
    //# Path validation succeeds when a PATH_RESPONSE frame is received that
    //# contains the data that was sent in a previous PATH_CHALLENGE frame.
    //# A PATH_RESPONSE frame received on any network path validates the path
    //# on which the PATH_CHALLENGE was sent.
    pub fn on_path_response(&mut self, path_id: Id, response: &s2n_quic_core::frame::PathResponse) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE was received.
        // This requirement is achieved because paths own their challenges.
        // We compare the path_response data to the data stored in the
        // receiving path's challenge.
        self[path_id].validate_path_response(response.data);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
    //# Tokens are
    //# invalidated when their associated connection ID is retired via a
    //# RETIRE_CONNECTION_ID frame (Section 19.16).
    pub fn on_connection_id_retire(&self, _connection_id: &connection::LocalId) {
        // TODO invalidate any tokens issued under this connection id
    }

    /// Called when a NEW_CONNECTION_ID frame is received from the peer
    pub fn on_new_connection_id(
        &mut self,
        connection_id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: &stateless_reset::Token,
    ) -> Result<(), transport::Error> {
        // Retire and register connection ID
        self.peer_id_registry.on_new_connection_id(
            connection_id,
            sequence_number,
            retire_prior_to,
            stateless_reset_token,
        )?;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //# Upon receipt of an increased Retire Prior To field, the peer MUST
        //# stop using the corresponding connection IDs and retire them with
        //# RETIRE_CONNECTION_ID frames before adding the newly provided
        //# connection ID to the set of active connection IDs.
        let active_path_connection_id = self.active_path().peer_connection_id;

        if !self.peer_id_registry.is_active(&active_path_connection_id) {
            self.active_path_mut().peer_connection_id =
                self.peer_id_registry.consume_new_id().expect(
                    "Since we are only checking the active path and new ID was delivered \
                    via the NEW_CONNECTION_ID frames, there will always be a new ID available \
                    to consume if necessary",
                );
        }

        Ok(())
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        for path in self.paths.iter_mut() {
            path.on_timeout(timestamp);
        }

        if !self.active_path().is_validated() && !self.active_path().is_challenge_pending(timestamp)
        {
            if let Some(last_known_validated_path) = self.last_known_validated_path {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                //# To protect the connection from failing due to such a spurious
                //# migration, an endpoint MUST revert to using the last validated peer
                //# address when validation of a new peer address fails.
                self.active = last_known_validated_path;
                self.last_known_validated_path = None;
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
        //= type=TODO
        //# If an endpoint has no state about the last validated peer address, it
        //# MUST close the connection silently by discarding all connection
        //# state.
    }

    /// Notifies the path manager of the connection closing event
    pub fn on_closing(&mut self) {
        self.active_path_mut().on_closing();
        // TODO clean up other paths
    }
}

impl<CCE: congestion_controller::Endpoint> transmission::interest::Provider for Manager<CCE> {
    fn transmission_interest(&self) -> transmission::Interest {
        // TODO get interest from pending paths
        self.peer_id_registry.transmission_interest()
    }
}

/// Internal Id of a path in the manager
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Id(u8);

impl Id {
    pub fn new(id: u8) -> Self {
        Self(id)
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::Index<Id> for Manager<CCE> {
    type Output = Path<CCE::CongestionController>;

    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.0 as usize]
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::IndexMut<Id> for Manager<CCE> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.0 as usize]
    }
}

#[allow(dead_code)]
pub struct PendingPaths<'a, CCE: congestion_controller::Endpoint> {
    index: usize,
    path_manager: &'a mut Manager<CCE>,
    timestamp: Timestamp,
}

impl<'a, CCE: congestion_controller::Endpoint> PendingPaths<'a, CCE> {
    pub fn new(path_manager: &'a mut Manager<CCE>, timestamp: Timestamp) -> Self {
        Self {
            index: 0,
            path_manager,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{ConnectionIdMapper, InternalConnectionIdGenerator};
    use core::time::Duration;
    use s2n_quic_core::{
        endpoint,
        inet::{DatagramInfo, ExplicitCongestionNotification},
        random::{self, Generator},
        recovery::{congestion_controller::testing::unlimited, RttEstimator},
        stateless_reset,
        stateless_reset::token::testing::*,
        time::{Clock, NoopClock},
    };
    use std::net::SocketAddr;

    // Helper function to easily create a PathManager
    fn manager(
        first_path: Path<unlimited::CongestionController>,
        stateless_reset_token: Option<stateless_reset::Token>,
    ) -> Manager<unlimited::Endpoint> {
        let mut random_generator = random::testing::Generator(123);
        let peer_id_registry =
            ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
                .create_peer_id_registry(
                    InternalConnectionIdGenerator::new().generate_id(),
                    first_path.peer_connection_id,
                    stateless_reset_token,
                );
        Manager::new(first_path, peer_id_registry)
    }

    #[test]
    fn get_path_by_address_test() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_local_conn_id = connection::LocalId::TEST_ID;
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );

        let second_conn_id = connection::PeerId::try_from_bytes(&[5, 4, 3, 2, 1]).unwrap();
        let second_path = Path::new(
            SocketAddress::default(),
            second_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );

        let mut manager = manager(first_path.clone(), None);
        manager.paths.push(second_path);
        assert_eq!(manager.paths.len(), 2);

        let (_id, matched_path) = manager.path(&SocketAddress::default()).unwrap();
        assert_eq!(
            matched_path.peer_connection_id,
            first_path.peer_connection_id
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
    //= type=test
    //# To protect the connection from failing due to such a spurious
    //# migration, an endpoint MUST revert to using the last validated peer
    //# address when validation of a new peer address fails.
    #[test]
    fn test_invalid_path_fallback() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_local_conn_id = connection::LocalId::TEST_ID;
        let mut first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        first_path.on_validated();

        // Create a challenge that will expire in 100ms
        let clock = NoopClock {};
        let expiration = Duration::from_millis(1000);
        let challenge = challenge::Challenge::new(clock.get_time() + expiration, [0; 8]);
        let second_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        )
        .with_challenge(challenge);

        let mut manager = manager(first_path, None);
        manager.paths.push(second_path);
        assert_eq!(manager.last_known_validated_path, None);
        assert_eq!(manager.active, 0);
        assert!(manager.paths[0].is_validated());

        manager.update_active_path(Id(1)).unwrap();
        assert_eq!(manager.active, 1);
        assert_eq!(manager.last_known_validated_path, Some(0));

        // After a validation times out, the path should revert to the previous
        manager.on_timeout(clock.get_time() + expiration + Duration::from_millis(100));
        assert_eq!(manager.active, 0);
        assert!(manager.last_known_validated_path.is_none());
    }

    #[test]
    // a validated path should be assigned to last_known_validated_path
    fn promote_validated_path_to_last_known_validated_path() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert_eq!(
            helper.manager.paths[helper.first_path_id.0 as usize].is_validated(),
            false
        );

        // Trigger:
        helper.manager.paths[helper.first_path_id.0 as usize].on_validated();
        assert_eq!(
            helper.manager.paths[helper.first_path_id.0 as usize].is_validated(),
            true
        );
        helper
            .manager
            .update_active_path(helper.second_path_id)
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.last_known_validated_path, Some(0));
    }

    #[test]
    // a NOT validated path should NOT be assigned to last_known_validated_path
    fn dont_promote_non_validated_path_to_last_known_validated_path() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert_eq!(
            helper.manager.paths[helper.first_path_id.0 as usize].is_validated(),
            false
        );

        // Trigger:
        helper
            .manager
            .update_active_path(helper.second_path_id)
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.last_known_validated_path, None);
    }

    #[test]
    // update path to the new active path
    fn update_path_to_active_path() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert_eq!(helper.manager.active, helper.first_path_id.0);

        // Trigger:
        helper
            .manager
            .update_active_path(helper.second_path_id)
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.active, helper.second_path_id.0);
    }

    #[test]
    // Don't update path to the new active path if insufficient connection ids
    fn dont_update_path_to_active_path_if_no_connection_id_available() {
        // Setup:
        let mut helper = helper_manager_register_second_path_conn_id(false);
        assert_eq!(helper.manager.active, helper.first_path_id.0);

        // Trigger:
        assert!(helper
            .manager
            .update_active_path(helper.second_path_id)
            .is_err());

        // Expectation:
        assert_eq!(helper.manager.active, helper.first_path_id.0);
    }

    #[test]
    fn test_path_validation() {
        let clock = NoopClock {};
        let mut path_rnd_generator = random::testing::Generator(123);
        let mut expected_data: [u8; 8] = [0; 8];
        path_rnd_generator.public_random_fill(&mut expected_data);

        // Create a challenge that will expire in 100ms
        let expiration = Duration::from_millis(100);
        let challenge = challenge::Challenge::new(clock.get_time() + expiration, expected_data);
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        )
        .with_challenge(challenge);

        let mut manager = manager(first_path.clone(), None);
        assert_eq!(manager.paths.len(), 1);

        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), false);
        } else {
            panic!("Path not found");
        }

        let frame = s2n_quic_core::frame::PathResponse {
            data: &expected_data,
        };

        // A response 100ms before the challenge should succeed
        manager.on_timeout(clock.get_time() + expiration - Duration::from_millis(100));

        manager.on_path_response(Id(0), &frame);
        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), true);
        } else {
            panic!("path not found");
        }
    }

    #[test]
    fn test_path_validation_abandoned_challenge() {
        let clock = NoopClock {};
        let mut path_rnd_generator = random::testing::Generator(123);
        let mut expected_data: [u8; 8] = [0; 8];
        path_rnd_generator.public_random_fill(&mut expected_data);

        // Create a challenge that will expire in 100ms
        let expiration = Duration::from_millis(100);
        let challenge = challenge::Challenge::new(clock.get_time() + expiration, expected_data);
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        )
        .with_challenge(challenge);

        let mut manager = manager(first_path.clone(), None);
        assert_eq!(manager.paths.len(), 1);

        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), false);
        } else {
            panic!("Path not found");
        }

        let frame = s2n_quic_core::frame::PathResponse {
            data: &expected_data,
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.4
        //= type=test
        //# Endpoints SHOULD abandon path validation based on a timer.

        // A response 100ms after the challenge should fail
        manager.on_timeout(clock.get_time() + expiration + Duration::from_millis(100));
        manager.on_path_response(Id(0), &frame);
        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), false);
        } else {
            panic!("path not found");
        }
    }

    #[test]
    // add new path when receiving a datagram on different remote address
    // Setup:
    // - create path manger with one path
    //
    // Trigger:
    // - call on_datagram_received with new remote address
    //
    // Expectation:
    // - assert we have two paths
    fn test_adding_new_path() {
        // Setup:
        let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let mut manager = manager(first_path, None);

        // verify we have one path
        assert_eq!(manager.path(&SocketAddress::default()).is_some(), true);
        let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        assert!(manager.path(&new_addr).is_none());
        assert_eq!(manager.paths.len(), 1);

        // Trigger:
        let datagram = DatagramInfo {
            timestamp: NoopClock {}.get_time(),
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };
        let (path_id, unblocked) = manager
            .on_datagram_received(
                &datagram,
                &connection::Limits::default(),
                true,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
            )
            .unwrap();

        // Expectation:
        assert_eq!(path_id.0, 1);
        assert_eq!(unblocked, false);
        assert_eq!(manager.path(&new_addr).is_some(), true);
        assert_eq!(manager.paths.len(), 2);
    }

    #[test]
    // do NOT add new path if handshake is not confirmed
    // Setup:
    // - create path manger with one path
    //
    // Trigger:
    // - call on_datagram_received with new remote address bit handshake_confirmed false
    //
    // Expectation:
    // - asset on_datagram_received errors
    // - assert we have one paths
    fn do_not_add_new_path_if_handshake_not_confirmed() {
        // Setup:
        let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let mut manager = manager(first_path, None);

        // verify we have one path
        let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        assert_eq!(manager.paths.len(), 1);

        // Trigger:
        let datagram = DatagramInfo {
            timestamp: NoopClock {}.get_time(),
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };
        let handshake_confirmed = false;
        let on_datagram_result = manager.on_datagram_received(
            &datagram,
            &connection::Limits::default(),
            handshake_confirmed,
            &mut unlimited::Endpoint::default(),
            &mut random::testing::Generator(123),
        );

        // Expectation:
        assert!(on_datagram_result.is_err());
        assert_eq!(manager.path(&new_addr).is_some(), false);
        assert_eq!(manager.paths.len(), 1);
    }

    // TODO remove early return statement when challenges work
    #[allow(unreachable_code)]
    #[test]
    fn connection_migration_challenge_behavior() {
        // Setup:
        let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let mut manager = manager(first_path, None);

        let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        let now = NoopClock {}.get_time();
        let datagram = DatagramInfo {
            timestamp: now,
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };

        let (_path_id, _unblocked) = manager
            .handle_connection_migration(
                &datagram,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
            )
            .unwrap();

        // verify we have two paths
        assert_eq!(manager.path(&new_addr).is_some(), true);
        assert_eq!(manager.paths.len(), 2);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //= type=TODO
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.
        return;
        assert!(manager[Id(1)].is_challenge_pending(now));

        let expired_challenge = now + Duration::from_millis(1_000);
        assert!(manager[Id(1)].is_challenge_pending(expired_challenge));

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //= type=test
        //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
        //# frame so that it can associate the peer's response with the
        //# corresponding PATH_CHALLENGE.
        // Verify that the data stored in the challenge is taken from the random generator
        // TODO does the below actually work?? investigate
        let mut test_rnd_generator = random::testing::Generator(123);
        let mut expected_data: [u8; 8] = [0; 8];
        test_rnd_generator.public_random_fill(&mut expected_data);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //= type=test
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.
        manager[Id(1)].validate_path_response(&expected_data);
        assert!(manager[Id(1)].is_validated());
    }

    #[test]
    // Abandon timer should use max PTO of active and new path(new path uses kInitialRtt)
    // Setup:
    // - create manager with path
    // - create datagram for packet on second path
    //
    // Trigger:
    // - call handle_connection_migration with packet for second path
    //
    // Expectation:
    // - assert that new path uses max_ack_delay from the active path
    //
    // Setup:
    // - modify rtt for fist path to detect difference in PTO
    //
    // Expectation:
    // - veify PTO of second path > PTO of first path
    // - verify abandon_timer uses PTO of second path (test with +- 10ms)
    // - verify abandon_timer does NOT uses PTO of first path (test with +- 10ms)
    fn connection_migration_new_path_abandon_timer() {
        // Setup:
        let first_path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[1]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let mut manager = manager(first_path, None);

        let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        let now = NoopClock {}.get_time();
        let datagram = DatagramInfo {
            timestamp: now,
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };

        // Trigger:
        let (second_path_id, _unblocked) = manager
            .handle_connection_migration(
                &datagram,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
            )
            .unwrap();
        let first_path_id = Id(0);

        // Expectation:
        // inherit max_ack_delay from the active path
        assert_eq!(manager.active, first_path_id.0);
        assert_eq!(
            &manager[first_path_id].rtt_estimator.max_ack_delay(),
            &manager[second_path_id].rtt_estimator.max_ack_delay()
        );

        // Setup:
        // modify rtt for first path so we can detect differences
        manager[first_path_id].rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );
        let first_path = &manager[first_path_id];
        let second_path = &manager[second_path_id];

        // Expectation:
        // verify the pto_period of the first path is less than the second path
        let first_path_pto = first_path.pto_period(PacketNumberSpace::ApplicationData);
        let second_path_pto = second_path.pto_period(PacketNumberSpace::ApplicationData);

        assert_eq!(first_path_pto, Duration::from_millis(330));
        assert_eq!(second_path_pto, Duration::from_millis(1_029));
        assert!(second_path_pto > first_path_pto);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#8.2.4
        //= type=test
        //# Endpoints SHOULD abandon path validation based on a timer.  When
        //# setting this timer, implementations are cautioned that the new path
        //# could have a longer round-trip time than the original. A value of
        //# three times the larger of the current Probe Timeout (PTO) or the PTO
        //# for the new path (that is, using kInitialRtt as defined in
        //# [QUIC-RECOVERY]) is RECOMMENDED.
        // abandon_duration should use max pto_period: second path
        let abandon_time = now + (second_path_pto * 3);
        assert_eq!(
            second_path.is_challenge_pending(abandon_time - Duration::from_millis(10)),
            true
        );
        assert_eq!(
            second_path.is_challenge_pending(abandon_time + Duration::from_millis(10)),
            false
        );

        // abandon_duration should NOT use lesser of pto_period: first path
        let not_abandon_time = now + (first_path_pto * 3); // using the minumum of the pto is not the correct abandon timer
        assert_eq!(
            second_path.is_challenge_pending(not_abandon_time - Duration::from_millis(10)),
            true
        );
        assert_eq!(
            second_path.is_challenge_pending(not_abandon_time + Duration::from_millis(10)),
            true
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
    //= type=test
    //# Upon receipt of an increased Retire Prior To field, the peer MUST
    //# stop using the corresponding connection IDs and retire them with
    //# RETIRE_CONNECTION_ID frames before adding the newly provided
    //# connection ID to the set of active connection IDs.
    #[test]
    fn stop_using_a_retired_connection_id() {
        let id_1 = connection::PeerId::try_from_bytes(b"id01").unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            id_1,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let mut manager = manager(first_path, None);

        let id_2 = connection::PeerId::try_from_bytes(b"id02").unwrap();
        assert!(manager
            .on_new_connection_id(&id_2, 1, 1, &TEST_TOKEN_1)
            .is_ok());

        assert_eq!(id_2, manager.paths[0].peer_connection_id);
    }

    fn helper_manager_register_second_path_conn_id(register_second_conn_id: bool) -> Helper {
        let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
        let second_conn_id = connection::PeerId::try_from_bytes(&[2]).unwrap();
        let local_conn_id = connection::LocalId::TEST_ID;
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );

        // Create a challenge that will expire in 100ms
        let now = NoopClock {}.get_time();
        let expiration = Duration::from_millis(100);
        let challenge = challenge::Challenge::new(now + expiration, [0; 8]);
        let second_path = Path::new(
            SocketAddress::default(),
            second_conn_id,
            local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        )
        .with_challenge(challenge);

        let mut random_generator = random::testing::Generator(123);
        let mut peer_id_registry =
            ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
                .create_peer_id_registry(
                    InternalConnectionIdGenerator::new().generate_id(),
                    first_path.peer_connection_id,
                    None,
                );
        if register_second_conn_id {
            assert!(peer_id_registry
                .on_new_connection_id(&second_conn_id, 1, 0, &TEST_TOKEN_2)
                .is_ok());
        }

        let mut manager = Manager::new(first_path, peer_id_registry);
        manager.paths.push(second_path);

        assert_eq!(manager.last_known_validated_path, None);
        assert_eq!(manager.active, 0);

        Helper {
            now,
            first_path_id: Id(0),
            second_path_id: Id(1),
            manager,
        }
    }

    fn helper_manager_with_paths() -> Helper {
        helper_manager_register_second_path_conn_id(true)
    }

    struct Helper {
        pub now: Timestamp,
        pub first_path_id: Id,
        pub second_path_id: Id,
        pub manager: Manager<unlimited::Endpoint>,
    }
}
