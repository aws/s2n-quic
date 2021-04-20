// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Manager implementation

use crate::{connection::PeerIdRegistry, transmission};
/// re-export core
pub use s2n_quic_core::path::*;
use s2n_quic_core::{
    ack, connection, frame,
    inet::{DatagramInfo, SocketAddress},
    packet::number::PacketNumberSpace,
    path::challenge::Challenge,
    random,
    recovery::{congestion_controller, CongestionController, RttEstimator},
    stateless_reset,
    time::{Duration, Timestamp},
    transport,
};
use smallvec::SmallVec;
//use transmission::path;

/// The amount of Paths that can be maintained without using the heap
const INLINE_PATH_LEN: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Debug)]
pub struct Manager<CCE: congestion_controller::Endpoint> {
    /// Path array
    paths: SmallVec<[Path; INLINE_PATH_LEN]>,

    /// Registry of `connection::PeerId`s
    peer_id_registry: PeerIdRegistry,

    /// Index to the active path
    active: usize,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
    //# To protect the connection from failing due to such a spurious
    //# migration, an endpoint MUST revert to using the last validated peer
    //# address when validation of a new peer address fails.
    /// Index of last known validated path
    previous: Option<usize>,

    /// The congestion controller for the path
    congestion_controller_endpoint: CCE,
    congestion_controller: CCE::CongestionController,
}

impl<CCE: CongestionController> Manager<CCE> {
    pub fn new(
        initial_path: Path,
        peer_id_registry: PeerIdRegistry,
        congestion_controller_endpoint: CCE,
    ) -> Self {
        let path_info = congestion_controller::PathInfo::new(&datagram.remote_address);
        Manager {
            paths: SmallVec::from_elem(initial_path, 1),
            peer_id_registry,
            active: 0,
            congestion_controller_endpoint.new_congestion_controller(),
            previous: None,
        }
    }

    /// Update the active path
    pub fn update_active_path(&mut self, path_id: Id) {
        // TODO return an error if the path doesn't exist
        // Or take an index and verify INLINE_PATH_LEN
        self.previous = Some(self.active);
        self.active = path_id.0;
    }

    /// Return the active path
    pub fn active_path(&self) -> &Path {
        &self.paths[self.active]
    }

    /// Return a mutable reference to the active path
    pub fn active_path_mut(&mut self) -> &mut Path {
        &mut self.paths[self.active]
    }

    /// Return the Id of the active path
    pub fn active_path_id(&self) -> Id {
        Id(self.active)
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path(&self, peer_address: &SocketAddress) -> Option<(Id, &Path)> {
        self.paths
            .iter()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id), path))
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path_mut(&mut self, peer_address: &SocketAddress) -> Option<(Id, &mut Path)> {
        self.paths
            .iter_mut()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id), path))
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

        // Since we are not currently supporting connection migration (whether it was deliberate or
        // not), we will error our at this point to avoid re-using a peer connection ID.
        // TODO: This would be better handled as a stateless reset so the peer can terminate the
        //       connection immediately. https://github.com/awslabs/s2n-quic/issues/317
        //return Err(transport::Error::INTERNAL_ERROR);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
        //= type=TODO
        //# Because port-only changes are commonly the
        //# result of NAT rebinding or other middlebox activity, the endpoint MAY
        //# instead retain its congestion control state and round-trip estimate
        //# in those cases instead of reverting to initial values.

        // TODO temporarily copy rtt and cc to maintain state when migrating to this new path.
        let rtt = self.active_path().rtt_estimator;
        let cc = self.active_path().congestion_controller.migrate_paths();

        //let rtt = RttEstimator::new(Duration::from_millis(250));

        //println!(" -- Creating new congestion controller");
        //let path_info = congestion_controller::PathInfo::new(&datagram.remote_address);
        //let cc = congestion_controller_endpoint.new_congestion_controller(path_info);
        // TODO grab a new conn id correctly,
        let conn_id = self.active_path().peer_connection_id;
        let new_id = self
            .peer_id_registry
            .consume_new_id_if_necessary(Some(&conn_id));

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
        let challenge = Challenge::new(
            datagram.timestamp,
            rtt.pto_period(1, PacketNumberSpace::ApplicationData),
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.4
            //= type=TODO
            //# A value of
            //# three times the larger of the current Probe Timeout (PTO) or the
            //# initial timeout (that is, 2*kInitialRtt) as defined in
            //# [QUIC-RECOVERY] is RECOMMENDED.
            rtt.pto_period(6, PacketNumberSpace::ApplicationData),
            datagram.remote_address,
            data,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
        //# Until a peer's address is deemed valid, an endpoint MUST
        //# limit the rate at which it sends data to this address.
        let path = Path::new_probe(
            datagram.remote_address,
            // TODO https://github.com/awslabs/s2n-quic/issues/316
            // The existing peer connection id may only be reused if the destination
            // connection ID on this packet had not been used before (this would happen
            // when the peer's remote address gets changed due to circumstances out of their
            // control). Otherwise we will need to consume a new connection::PeerId by calling
            // PeerIdRegistry::consume_new_id_if_necessary(None) and ignoring the request if
            // no new connection::PeerId is available to use.
            new_id.unwrap(),
            rtt,
            cc,
            true,
            challenge,
        );
        let id = Id(self.paths.len());
        self.paths.push(path);
        println!(" -- New path pushedD");

        Ok((id, false))
    }

    pub fn next_timer(&self) -> impl Iterator<Item = Timestamp> + '_ {
        self.paths.iter().flat_map(|p| p.next_timer())
    }

    /// Writes any frames the path manager wishes to transmit to the given context
    pub fn on_transmit<W: transmission::WriteContext>(&mut self, context: &mut W) {
        self.peer_id_registry.on_transmit(context);

        let constraint = context.transmission_constraint();
        if constraint.can_transmit() {
            let path = &mut self.paths[context.path_id().0];

            if path.is_challenge_pending(context.current_time()) {
                println!(" -- Sending challenge");
                if let Some(data) = path.challenge_data() {
                    let frame = frame::PathChallenge { data };
                    context.write_frame(&frame);
                    self[context.path_id()].reset_timer(context.current_time());
                }
            }
        }
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
        peer_address: &SocketAddress,
        response: &s2n_quic_core::frame::PathResponse,
    ) {
        if let Some((_id, path)) = self.path_mut(peer_address) {
            if path.is_validated() {
                return;
            }

            if path.is_path_response_valid(timestamp, peer_address, response.data) {
                println!(" -- HEY THEY RESPONSE IS VALID");
                path.on_validated();

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
                //= type=TODO
                //# After verifying a new client address, the server SHOULD send new
                //# address validation tokens (Section 8) to the client.
            }
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

        // TODO This new connection ID may retire IDs in use by multiple paths. Since we are not
        //      currently supporting connection migration, there is only one path, but once there
        //      are more than one we should decide what to do if there aren't enough new connection
        //      IDs available for all paths.
        //      See https://github.com/awslabs/s2n-quic/issues/358
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //# Upon receipt of an increased Retire Prior To field, the peer MUST
        //# stop using the corresponding connection IDs and retire them with
        //# RETIRE_CONNECTION_ID frames before adding the newly provided
        //# connection ID to the set of active connection IDs.
        // Ensure all paths are not using a newly retired connection ID
        for path in self.paths.iter_mut() {
            path.peer_connection_id = self
                .peer_id_registry
                .consume_new_id_if_necessary(Some(&path.peer_connection_id))
                .expect(
                    "There is only one path maintained currently and since a new ID was \
                delivered, there will always be a new ID available to consume if necessary",
                );
        }

        Ok(())
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        for path in self.paths.iter_mut() {
            path.on_timeout(timestamp);
        }

        if !self.active_path().is_validated() && self.active_path().is_challenge_abandoned() {
            if let Some(previous) = self.previous {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                //# To protect the connection from failing due to such a spurious
                //# migration, an endpoint MUST revert to using the last validated peer
                //# address when validation of a new peer address fails.
                self.active = previous;
                self.previous = None;
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
        let interest = self.peer_id_registry.transmission_interest();

        for p in &self.paths {
            if !p.is_validated() {
                // TODO Is this the right interest to express?
                // interest += transmission::interest::Interest::Forced;
            }
        }

        interest
    }
}

/// Internal Id of a path in the manager
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Id(usize);

impl Id {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::Index<Id> for Manager<CCE> {
    type Output = Path;

    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.0]
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::IndexMut<Id> for Manager<CCE> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.0]
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
            let path = self.path_manager.paths.get(self.index)?;

            // We have to advance the index before returning or we risk
            // returning the same path over and over.
            self.index += 1;

            if path.is_challenge_pending(self.timestamp) {
                return Some((Id(self.index - 1), self.path_manager));
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
        first_path: Path,
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
        Manager::new(first_path, peer_id_registry, unlimited::Endpoint::default())
    }

    #[test]
    fn get_path_by_address_test() {
        let conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            false,
        );

        let manager = manager(first_path.clone(), None);
        assert_eq!(manager.paths.len(), 1);

        let (_id, matched_path) = manager.path(&SocketAddress::default()).unwrap();
        assert_eq!(matched_path, &first_path);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
    //= type=test
    //# To protect the connection from failing due to such a spurious
    //# migration, an endpoint MUST revert to using the last validated peer
    //# address when validation of a new peer address fails.
    #[test]
    fn test_invalid_path_fallback() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
        );

        // Create a challenge that will expire in 100ms
        let clock = NoopClock {};
        let expiration = Duration::from_millis(100);
        let challenge = Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            SocketAddress::default(),
            [0; 8],
        );
        let second_path = Path::new_probe(
            SocketAddress::default(),
            first_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            challenge,
        );

        let mut manager = manager(first_path, None);
        manager.paths.push(second_path);
        assert_eq!(manager.previous, None);
        assert_eq!(manager.active, 0);
        manager.update_active_path(Id(1));
        assert_eq!(manager.previous, Some(0));
        assert_eq!(manager.active, 1);

        // After a validation times out, the path should revert to the previous
        manager.on_timeout(clock.get_time() + Duration::from_millis(2000));
        assert!(manager.previous.is_none());
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
        let challenge = Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            SocketAddress::default(),
            expected_data,
        );
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new_probe(
            SocketAddress::default(),
            first_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            challenge,
        );

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
            &first_path.peer_socket_address,
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
            &first_path.peer_socket_address,
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
            RttEstimator::new(Duration::from_millis(30)),
            false,
        );
        let manager = manager(first_path, None);
        assert_eq!(manager.paths.len(), 1);
        assert_eq!(manager.path(&SocketAddress::default()).is_some(), true);

        let new_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        assert_eq!(manager.path(&new_addr), None);
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

        let mut random_generator = random::testing::Generator(123);
        let (_path_id, _unblocked) = manager
            .on_datagram_received(
                &datagram,
                &connection::Limits::default(),
                true,
                &mut unlimited::Endpoint::default(),
                &mut random_generator,
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
                    &mut random_generator,
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
            RttEstimator::new(Duration::from_millis(30)),
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
