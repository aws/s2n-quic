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
        // Attempt to consume a new connection id incase it has been retired since the last use.
        let peer_connection_id = self
            .paths
            .get(new_path_idx as usize)
            .map(|x| &x.peer_connection_id);

        // The path's connection id might have retired since we last used it. Check if it is still
        // active, otherwise try and consume a new connection id.
        let use_peer_connection_id = match peer_connection_id {
            Some(peer_connection_id) => {
                if self.peer_id_registry.is_active(peer_connection_id) {
                    *peer_connection_id
                } else {
                    // FIXME https://github.com/awslabs/s2n-quic/issues/669
                    // If there are no new connection ids the peer is responsible for
                    // providing additional connection ids to continue.
                    //
                    // Insufficient connection ids should not cause the connection to close.
                    // Replace with an error code that is silently ignored.
                    self.peer_id_registry
                        .consume_new_id()
                        .ok_or(transport::Error::INTERNAL_ERROR)?
                }
            }
            None => panic!("the path attempting to become the active path does not exist"),
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

    /// Return a mutable reference to the active path
    pub fn path_mut_by_id(&mut self, path_id: Id) -> Option<&mut Path<CCE::CongestionController>> {
        self.paths.get_mut(path_id.0 as usize)
    }

    /// Called when a datagram is received on a connection
    /// Upon success, returns a `(Id, bool)` containing the path ID and a boolean that is
    /// true if the path had been amplification limited prior to receiving the datagram
    /// and is now no longer amplification limited.
    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    pub fn on_datagram_received<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        limits: &connection::Limits,
        can_migrate: bool,
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
        if !can_migrate {
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
                    // Replace with an error code that is silently ignored.
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

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.6.3
        //# Servers SHOULD initiate path validation to the client's new address
        //# upon receiving a probe packet from a different address.
        // This will overwrite any in-progress path validation
        let challenge = challenge::Challenge::new(
            datagram.timestamp,
            rtt.pto_period(1, PacketNumberSpace::ApplicationData),
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#8.2.4
            //= type=TODO
            //# A value of
            //# three times the larger of the current Probe Timeout (PTO) or the PTO
            //# for the new path (that is, using kInitialRtt as defined in
            //# [QUIC-RECOVERY]) is RECOMMENDED.

            //
            //# three times the larger of the current Probe Timeout (PTO) or the
            //# initial timeout (that is, 2*kInitialRtt) as defined in
            //# [QUIC-RECOVERY] is RECOMMENDED.
            //
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
            //= type=TODO
            //= tracking-issue=https://github.com/awslabs/s2n-quic/issues/412
            //# This timer SHOULD be set as described in Section 6.2.1 of
            //# [QUIC-RECOVERY] and MUST NOT be more aggressive.
            rtt.pto_period(6, PacketNumberSpace::ApplicationData),
            data,
        );

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
        )
        .with_challenge(challenge);

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
    pub fn on_path_response(
        &mut self,
        timestamp: Timestamp,
        path_id: Id,
        response: &s2n_quic_core::frame::PathResponse,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE was received.
        // This requirement is achieved because paths own their challenges.
        // We compare the path_response data to the data stored in the
        // receiving path's challenge.
        if let Some(path) = self.path_mut_by_id(path_id) {
            if !path.is_validated() {
                path.validate_path_response(timestamp, response.data);
            }
        } else {
            // TODO we should not have gotten a PATH_RESPONSE for a path that
            // does not exist. Check if we drop the frame or if we error.
        }
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
        // Register the new connection ID
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
                    via the NEW_CONNECTION_ID frams, there will always be a new ID available \
                    to consume if necessary",
                );
        }

        Ok(())
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        for path in self.paths.iter_mut() {
            path.on_timeout(timestamp);
        }

        if !self.active_path().is_validated() && self.active_path().is_challenge_abandoned() {
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

    pub fn next_path(&mut self) -> Option<(Id, &mut Manager<CCE>)> {
        loop {
            let index = self.index;
            self.index += 1;

            let path = self.path_manager.paths.get(index)?;

            if path.is_challenge_pending(self.timestamp) {
                return Some((Id(index as u8), self.path_manager));
            }
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
        stateless_reset::token::testing::TEST_TOKEN_1,
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
        let expiration = Duration::from_millis(100);
        let challenge = challenge::Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            [0; 8],
        );
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
        manager.on_timeout(clock.get_time() + Duration::from_millis(2000));
        assert!(manager.last_known_validated_path.is_none());
        assert_eq!(manager.active, 0);
    }

    #[test]
    fn test_path_validation() {
        let clock = NoopClock {};
        let mut path_rnd_generator = random::testing::Generator(123);
        let mut expected_data: [u8; 8] = [0; 8];
        path_rnd_generator.public_random_fill(&mut expected_data);

        // Create a challenge that will expire in 100ms
        let expiration = Duration::from_millis(100);
        let challenge = challenge::Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            expected_data,
        );
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
        manager.on_path_response(
            clock.get_time() + expiration + Duration::from_millis(100),
            Id(0),
            &frame,
        );
        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), false);
        } else {
            panic!("path not found");
        }

        // A response 100ms before the challenge should succeed
        manager.on_path_response(
            clock.get_time() + expiration - Duration::from_millis(100),
            Id(0),
            &frame,
        );
        if let Some((_path_id, first_path)) = manager.path(&first_path.peer_socket_address) {
            assert_eq!(first_path.is_validated(), true);
        } else {
            panic!("path not found");
        }
    }

    #[test]
    #[allow(unreachable_code)]
    fn test_new_peer() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );
        let manager = manager(first_path, None);
        assert_eq!(manager.paths.len(), 1);
        assert_eq!(manager.path(&SocketAddress::default()).is_some(), true);

        let new_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        assert!(manager.path(&new_addr).is_none());
        assert_eq!(manager.paths.len(), 1);

        // TODO Remove when Connection Migration is supported
        return;

        let clock = NoopClock {};
        let datagram = DatagramInfo {
            timestamp: clock.get_time(),
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };

        // NOTE This generator should be passed to on_datagram_received when migation is enabled
        let mut _random_generator = random::testing::Generator(123);
        let (_path_id, _unblocked) = manager
            .on_datagram_received(
                &datagram,
                &connection::Limits::default(),
                true,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
            )
            .unwrap();

        assert_eq!(manager.path(&new_addr).is_some(), true);
        assert_eq!(manager.paths.len(), 2);
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //= type=test
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.
        let timer = clock.get_time() + Duration::from_millis(2000);
        assert!(manager[Id(1)].is_challenge_pending(timer));

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //= type=test
        //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
        //# frame so that it can associate the peer's response with the
        //# corresponding PATH_CHALLENGE.
        // Verify that the data stored in the challenge is taken from the random generator
        let mut test_rnd_generator = random::testing::Generator(123);
        let mut expected_data: [u8; 8] = [0; 8];
        test_rnd_generator.public_random_fill(&mut expected_data);

        assert_eq!(
            manager[Id(1)].challenge_data().unwrap(),
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
            //= type=test
            //# An endpoint MUST
            //# perform path validation (Section 8.2) if it detects any change to a
            //# peer's address, unless it has previously validated that address.
            &expected_data
        );

        let new_addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        let datagram = DatagramInfo {
            timestamp: clock.get_time(),
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
        };

        // Verify an unconfirmed handshake does not add a new path
        assert_eq!(
            manager
                .on_datagram_received(
                    &datagram,
                    &connection::Limits::default(),
                    false,
                    &mut unlimited::Endpoint::default(),
                    &mut random::testing::Generator(123),
                )
                .is_err(),
            true
        );
        assert_eq!(manager.paths.len(), 2);
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
}
