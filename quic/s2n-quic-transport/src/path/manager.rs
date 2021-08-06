// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Manager implementation

use crate::{
    connection::PeerIdRegistry,
    path::{challenge, Path},
    transmission,
};
use s2n_quic_core::{
    ack, connection, event, frame,
    inet::{DatagramInfo, SocketAddress},
    packet::number::PacketNumberSpace,
    path::MaxMtu,
    random,
    recovery::{congestion_controller, RttEstimator},
    stateless_reset,
    time::{timer, Timestamp},
    transport,
};
use smallvec::SmallVec;

/// The amount of Paths that can be maintained without using the heap.
/// This value is also used to limit the number of connection migrations.
const MAX_ALLOWED_PATHS: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Debug)]
pub struct Manager<CCE: congestion_controller::Endpoint> {
    /// Path array
    paths: SmallVec<[Path<CCE::CongestionController>; MAX_ALLOWED_PATHS]>,

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
    fn update_active_path<Rnd: random::Generator, Pub: event::Publisher>(
        &mut self,
        path_id: Id,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(path_id != Id(self.active));

        let new_path_idx = path_id.as_u8();
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

        publisher.on_active_path_updated(event::builders::ActivePathUpdated {
            src_addr: &self.active_path().peer_socket_address,
            src_cid: &self.active_path().peer_connection_id,
            src_path_id: self.active as u64,
            dst_cid: &self[path_id].peer_connection_id,
            dst_addr: &self[path_id].peer_socket_address,
            dst_path_id: new_path_idx as u64,
        });

        if self.active_path().is_validated() {
            self.last_known_validated_path = Some(self.active);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.3
        //# In response to an apparent migration, endpoints MUST validate the
        //# previously active path using a PATH_CHALLENGE frame.
        //
        // TODO: https://github.com/awslabs/s2n-quic/issues/711
        // The usage of 'apparent' is vague and its not clear if the previous path should
        // always be validated or only if the new active path is not validated.
        if !self.active_path().is_challenge_pending() {
            self.set_challenge(self.active_path_id(), random_generator);
        }

        self.active = new_path_idx;
        Ok(())
    }

    /// Return the active path
    #[inline]
    pub fn active_path(&self) -> &Path<CCE::CongestionController> {
        &self.paths[self.active as usize]
    }

    /// Return a mutable reference to the active path
    #[inline]
    pub fn active_path_mut(&mut self) -> &mut Path<CCE::CongestionController> {
        &mut self.paths[self.active as usize]
    }

    /// Return the Id of the active path
    #[inline]
    pub fn active_path_id(&self) -> Id {
        Id(self.active)
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
    //= type=TODO
    //= tracking-issue=714
    //# An endpoint MAY skip validation of a peer address if
    //# that address has been seen recently.
    /// Returns the Path for the provided address if the PathManager knows about it
    #[inline]
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
    #[inline]
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

    /// Returns an iterator over all paths pending path_challenge or path_response
    /// transmission.
    pub fn paths_pending_validation(&mut self) -> PathsPendingValidation<CCE> {
        PathsPendingValidation::new(self)
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
        max_mtu: MaxMtu,
    ) -> Result<(Id, bool), transport::Error> {
        if let Some((id, path)) = self.path_mut(&datagram.remote_address) {
            let unblocked = path.on_bytes_received(datagram.payload_len);
            return Ok((id, unblocked));
        }

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

        self.handle_connection_migration(
            datagram,
            congestion_controller_endpoint,
            random_generator,
            max_mtu,
        )
    }

    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    fn handle_connection_migration<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut CCE,
        random_generator: &mut Rnd,
        max_mtu: MaxMtu,
    ) -> Result<(Id, bool), transport::Error> {
        // Since we are not currently supporting connection migration (whether it was deliberate or
        // not), we will error our at this point to avoid re-using a peer connection ID.
        // TODO: This would be better handled as a stateless reset so the peer can terminate the
        //       connection immediately. https://github.com/awslabs/s2n-quic/issues/317
        // We only enable connection migration for testing
        #[cfg(not(any(feature = "connection_migration", feature = "testing", test)))]
        return Err(
            transport::Error::INTERNAL_ERROR.with_reason("Connection Migration is not supported")
        );

        let new_path_idx = self.paths.len();
        // TODO: Support deletion of old paths: https://github.com/awslabs/s2n-quic/issues/741
        // The current path manager implementation does not delete or reuse indices
        // in the path array. This can result in an unbounded number of paths. To prevent
        // this we limit the max number of paths per connection.
        if new_path_idx >= MAX_ALLOWED_PATHS {
            return Err(transport::Error::INTERNAL_ERROR
                .with_reason("exceeded the max allowed paths per connection"));
        }
        let new_path_id = Id(new_path_idx as u8);

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
                    .ok_or_else(|| {
                        transport::Error::INTERNAL_ERROR.with_reason("insufficient connection ids")
                    })?
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

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.1
        //# Until a peer's address is deemed valid, an endpoint MUST
        //# limit the rate at which it sends data to this address.
        //
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
        //# An endpoint MAY send data to an unvalidated peer address, but it MUST
        //# protect against potential attacks as described in Section 9.3.1 and
        //# Section 9.3.2.
        //
        // New paths start in AmplificationLimited state until they are validated.
        let mut path = Path::new(
            datagram.remote_address,
            peer_connection_id,
            datagram.destination_connection_id,
            rtt,
            cc,
            true,
            max_mtu,
        );

        let unblocked = path.on_bytes_received(datagram.payload_len);
        // create a new path
        self.paths.push(path);
        self.set_challenge(new_path_id, random_generator);

        Ok((new_path_id, unblocked))
    }

    fn set_challenge<Rnd: random::Generator>(&mut self, path_id: Id, random_generator: &mut Rnd) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
        //# frame so that it can associate the peer's response with the
        //# corresponding PATH_CHALLENGE.
        let mut data: challenge::Data = [0; 8];
        random_generator.public_random_fill(&mut data);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.4
        //# Endpoints SHOULD abandon path validation based on a timer.
        //
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#8.2.4
        //# When
        //# setting this timer, implementations are cautioned that the new path
        //# could have a longer round-trip time than the original. A value of
        //# three times the larger of the current Probe Timeout (PTO) or the PTO
        //# for the new path (that is, using kInitialRtt as defined in
        //# [QUIC-RECOVERY]) is RECOMMENDED.
        let abandon_duration = self[path_id].pto_period(PacketNumberSpace::ApplicationData);
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
        let challenge = challenge::Challenge::new(abandon_duration, data);
        self[path_id].set_challenge(challenge);
    }

    /// Writes any frames the path manager wishes to transmit to the given context
    #[inline]
    pub fn on_transmit<W: transmission::WriteContext>(&mut self, context: &mut W) {
        self.peer_id_registry.on_transmit(context)

        // TODO Add in per-path constraints based on whether a Challenge needs to be
        // transmitted.
    }

    /// Called when packets are acknowledged
    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.peer_id_registry.on_packet_ack(ack_set);
    }

    /// Called when packets are lost
    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.peer_id_registry.on_packet_loss(ack_set);
    }

    #[inline]
    pub fn on_path_challenge(
        &mut self,
        peer_address: &SocketAddress,
        challenge: &frame::path_challenge::PathChallenge,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE was received.
        if let Some((_id, path)) = self.path_mut(peer_address) {
            path.on_path_challenge(challenge.data)
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.3
    //# Path validation succeeds when a PATH_RESPONSE frame is received that
    //# contains the data that was sent in a previous PATH_CHALLENGE frame.
    //# A PATH_RESPONSE frame received on any network path validates the path
    //# on which the PATH_CHALLENGE was sent.
    #[inline]
    pub fn on_path_response(&mut self, response: &frame::PathResponse) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE was received.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //# This requirement MUST NOT be enforced by the endpoint that initiates
        //# path validation, as that would enable an attack on migration; see
        //# Section 9.3.3.
        //
        // The 'attack on migration' refers to the following scenario:
        // If the packet forwarded by the off-attacker is received before the
        // genuine packet, the genuine packet will be discarded as a duplicate
        // and path validation will fail.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.3
        //# A PATH_RESPONSE frame received on any network path validates the path
        //# on which the PATH_CHALLENGE was sent.

        for path in self.paths.iter_mut() {
            path.on_path_response(response.data);
        }
    }

    /// Process a non-probing (path validation probing) packet.
    pub fn on_non_path_validation_probing_packet<Rnd: random::Generator, Pub: event::Publisher>(
        &mut self,
        path_id: Id,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        if self.active_path_id() != path_id {
            self.update_active_path(path_id, random_generator, publisher)?;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
            //# After changing the address to which it sends non-probing packets, an
            //# endpoint can abandon any path validation for other addresses.
            //
            // Abandon other path validations only if the active path is validated since an
            // attacker could block all path validation attempts simply by forwarding packets.
            if self.active_path().is_validated() {
                self.abandon_all_path_challenges();
            } else if !self.active_path().is_challenge_pending() {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
                //# If the recipient permits the migration, it MUST send subsequent
                //# packets to the new peer address and MUST initiate path validation
                //# (Section 8.2) to verify the peer's ownership of the address if
                //# validation is not already underway.
                self.set_challenge(self.active_path_id(), random_generator);
            }
        }
        Ok(())
    }

    #[inline]
    fn abandon_all_path_challenges(&mut self) {
        for path in self.paths.iter_mut() {
            path.abandon_challenge();
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

    pub fn on_timeout(&mut self, timestamp: Timestamp) -> Result<(), connection::Error> {
        for path in self.paths.iter_mut() {
            path.on_timeout(timestamp);
        }

        if self.active_path().failed_validation() {
            match self.last_known_validated_path {
                Some(last_known_validated_path) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                    //# To protect the connection from failing due to such a spurious
                    //# migration, an endpoint MUST revert to using the last validated peer
                    //# address when validation of a new peer address fails.
                    self.active = last_known_validated_path;
                    self.last_known_validated_path = None;
                }
                None => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
                    //# When an endpoint has no validated path on which to send packets, it
                    //# MAY discard connection state.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
                    //= type=TODO
                    //= tracking-issue=713
                    //# An endpoint capable of connection
                    //# migration MAY wait for a new path to become available before
                    //# discarding connection state.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                    //# If an endpoint has no state about the last validated peer address, it
                    //# MUST close the connection silently by discarding all connection
                    //# state.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
                    //# An endpoint MAY discard connection state if it does not have a
                    //# validated path on which it can send packets; see Section 8.2
                    return Err(connection::Error::NoValidPath);
                }
            }
        }

        Ok(())
    }

    /// Notifies the path manager of the connection closing event
    pub fn on_closing(&mut self) {
        self.active_path_mut().on_closing();
        // TODO clean up other paths
    }

    /// true if ALL paths are amplification_limited
    #[inline]
    pub fn is_amplification_limited(&self) -> bool {
        self.paths
            .iter()
            .all(|path| path.transmission_constraint().is_amplification_limited())
    }

    /// true if ANY of the paths can transmit
    #[inline]
    pub fn can_transmit(&self, interest: transmission::Interest) -> bool {
        self.paths.iter().any(|path| {
            let constraint = path.transmission_constraint();
            interest.can_transmit(constraint)
        })
    }

    #[inline]
    pub fn transmission_constraint(&self) -> transmission::Constraint {
        // Return the lowest constraint which will ensure we don't get blocked on transmission by a single path
        self.paths
            .iter()
            .map(|path| path.transmission_constraint())
            .min()
            .unwrap_or(transmission::Constraint::None)
    }
}

impl<CCE: congestion_controller::Endpoint> timer::Provider for Manager<CCE> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        for path in self.paths.iter() {
            path.timers(query)?;
        }

        Ok(())
    }
}

/// Iterate over all paths that have an interest in sending PATH_CHALLENGE
/// or PATH_RESPONSE frames.
///
/// This abstraction allows for iterating over pending paths while also
/// having mutable access to the Manager.
pub struct PathsPendingValidation<'a, CCE: congestion_controller::Endpoint> {
    index: u8,
    path_manager: &'a mut Manager<CCE>,
}

impl<'a, CCE: congestion_controller::Endpoint> PathsPendingValidation<'a, CCE> {
    pub fn new(path_manager: &'a mut Manager<CCE>) -> Self {
        Self {
            index: 0,
            path_manager,
        }
    }

    #[inline]
    pub fn next_path(&mut self) -> Option<(Id, &mut Manager<CCE>)> {
        loop {
            let path = self.path_manager.paths.get(self.index as usize)?;

            // Advance the index otherwise this will continue to process the
            // same path.
            self.index += 1;

            if path.is_challenge_pending() || path.is_response_pending() {
                return Some((Id(self.index - 1), self.path_manager));
            }
        }
    }
}

impl<CCE: congestion_controller::Endpoint> transmission::interest::Provider for Manager<CCE> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.peer_id_registry.transmission_interest(query)?;

        for path in self.paths.iter() {
            // query PATH_CHALLENGE and PATH_RESPONSE interest for each path
            path.transmission_interest(query)?;
        }

        Ok(())
    }
}

/// Internal Id of a path in the manager
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Id(u8);

impl Id {
    pub fn new(id: u8) -> Self {
        Self(id)
    }

    pub fn as_u8(&self) -> u8 {
        self.0
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::Index<Id> for Manager<CCE> {
    type Output = Path<CCE::CongestionController>;

    #[inline]
    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.0 as usize]
    }
}

impl<CCE: congestion_controller::Endpoint> core::ops::IndexMut<Id> for Manager<CCE> {
    #[inline]
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        path::DEFAULT_MAX_MTU,
    };
    use core::time::Duration;
    use s2n_quic_core::{
        endpoint,
        event::testing::Publisher,
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
            DEFAULT_MAX_MTU,
        );

        let second_conn_id = connection::PeerId::try_from_bytes(&[5, 4, 3, 2, 1]).unwrap();
        let second_path = Path::new(
            SocketAddress::default(),
            second_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
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
            DEFAULT_MAX_MTU,
        );
        // simulate receiving a handshake packet to force path validation
        first_path.on_handshake_packet();

        // Create a challenge that will expire in 100ms
        let now = NoopClock {}.get_time();
        let expiration = Duration::from_millis(1000);
        let challenge = challenge::Challenge::new(expiration, [0; 8]);
        let mut second_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            first_local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        second_path.set_challenge(challenge);

        let mut manager = manager(first_path, None);
        manager.paths.push(second_path);
        assert_eq!(manager.last_known_validated_path, None);
        assert_eq!(manager.active, 0);
        assert!(manager.paths[0].is_validated());

        manager
            .update_active_path(Id(1), &mut random::testing::Generator(123), &mut Publisher)
            .unwrap();
        assert_eq!(manager.active, 1);
        assert_eq!(manager.last_known_validated_path, Some(0));

        // send challenge and arm abandon timer
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        manager[Id(1)].on_transmit(&mut context);

        // After a validation times out, the path should revert to the previous
        manager
            .on_timeout(now + expiration + Duration::from_millis(100))
            .unwrap();
        assert_eq!(manager.active, 0);
        assert!(manager.last_known_validated_path.is_none());
    }

    #[test]
    // a validated path should be assigned to last_known_validated_path
    fn promote_validated_path_to_last_known_validated_path() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(!helper.manager.paths[helper.first_path_id.0 as usize].is_validated());

        // Trigger:
        // simulate receiving a handshake packet to force path validation
        helper.manager.paths[helper.first_path_id.0 as usize].on_handshake_packet();
        assert!(helper.manager.paths[helper.first_path_id.0 as usize].is_validated());
        helper
            .manager
            .update_active_path(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.last_known_validated_path, Some(1));
    }

    #[test]
    // a NOT validated path should NOT be assigned to last_known_validated_path
    fn dont_promote_non_validated_path_to_last_known_validated_path() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(!helper.manager.paths[helper.first_path_id.0 as usize].is_validated());

        // Trigger:
        helper
            .manager
            .update_active_path(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.last_known_validated_path, Some(0));
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
            .update_active_path(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.active, helper.second_path_id.0);
    }

    #[test]
    // Don't update path to the new active path if insufficient connection ids
    fn dont_update_path_to_active_path_if_no_connection_id_available() {
        // Setup:
        let mut helper = helper_manager_with_paths_base(false, true);
        assert_eq!(helper.manager.active, helper.first_path_id.0);

        // Trigger:
        assert_eq!(
            helper.manager.update_active_path(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher
            ),
            Err(transport::Error::INTERNAL_ERROR)
        );

        // Expectation:
        assert_eq!(helper.manager.active, helper.first_path_id.0);
    }

    #[test]
    fn set_path_challenge_on_active_path_on_connection_migration() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        helper.manager[helper.zero_path_id].abandon_challenge();
        assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
        assert_eq!(
            helper.manager.last_known_validated_path.unwrap(),
            helper.zero_path_id.0
        );

        // Trigger:
        helper
            .manager
            .update_active_path(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());
    }

    #[test]
    // A path should be validated if a PATH_RESPONSE contains data sent
    // via PATH_CHALLENGE
    //
    // Setup:
    // - create manager with 2 paths
    // - first path is active
    // - second path is pending challenge/validation
    //
    // Trigger 1:
    // - call on_timeout just BEFORE challenge should expire
    //
    // Expectation 1:
    // - verify second path is pending challenge
    // - verify second path is NOT validated
    //
    // Trigger 2:
    // - call on_path_response with expected data for second path
    //
    // Expectation 2:
    // - verify second path is validated
    fn validate_path_before_challenge_expiration() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert_eq!(helper.manager.active, helper.first_path_id.0);

        // send challenge and arm abandon timer
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        helper.manager[helper.second_path_id].on_transmit(&mut context);
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());

        // Trigger 1:
        // A response 100ms before the challenge is abandoned
        helper
            .manager
            .on_timeout(helper.now + helper.challenge_expiration - Duration::from_millis(100))
            .unwrap();

        // Expectation 1:
        assert!(helper.manager[helper.second_path_id].is_challenge_pending(),);
        assert!(!helper.manager[helper.second_path_id].is_validated());

        // Trigger 2:
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
        //= type=test
        //# This requirement MUST NOT be enforced by the endpoint that initiates
        //# path validation, as that would enable an attack on migration; see
        //# Section 9.3.3.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.3
        //= type=test
        //# A PATH_RESPONSE frame received on any network path validates the path
        //# on which the PATH_CHALLENGE was sent.
        //
        // The above requirements are satisfied because on_path_response is a path
        // agnostic function
        let frame = s2n_quic_core::frame::PathResponse {
            data: &helper.expected_data,
        };
        helper.manager.on_path_response(&frame);

        // Expectation 2:
        assert!(helper.manager[helper.second_path_id].is_validated());
    }

    #[test]
    // A path should NOT be validated if the challenge has been abandoned
    //
    // Setup:
    // - create manager with 2 paths
    // - first path is active
    // - second path is pending challenge/validation
    //
    // Trigger 1:
    // - call on_timeout just AFTER challenge should expire
    //
    // Expectation 1:
    // - verify second path is NOT pending challenge
    // - verify second path is NOT validated
    //
    //
    // Trigger 2:
    // - call on_path_response with expected data for second path
    //
    // Expectation 2:
    // - verify second path is NOT validated
    fn dont_validate_path_if_path_challenge_is_abandoned() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert_eq!(helper.manager.active, helper.first_path_id.0);

        // send challenge and arm abandon timer
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        helper.manager[helper.second_path_id].on_transmit(&mut context);
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());

        // Trigger 1:
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.4
        //= type=test
        //# Endpoints SHOULD abandon path validation based on a timer.
        // A response 100ms after the challenge should fail
        helper
            .manager
            .on_timeout(helper.now + helper.challenge_expiration + Duration::from_millis(100))
            .unwrap();

        // Expectation 1:
        assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.second_path_id].is_validated());

        // Trigger 2:
        let frame = s2n_quic_core::frame::PathResponse {
            data: &helper.expected_data,
        };
        helper.manager.on_path_response(&frame);

        // Expectation 2:
        assert!(!helper.manager[helper.second_path_id].is_validated());
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
    //# If the recipient permits the migration, it MUST send subsequent
    //# packets to the new peer address and MUST initiate path validation
    //# (Section 8.2) to verify the peer's ownership of the address if
    //# validation is not already underway.
    fn initiate_path_challenge_if_new_path_is_not_validated() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(!helper.manager[helper.first_path_id].is_validated());
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());

        assert!(!helper.manager[helper.second_path_id].is_validated());
        helper.manager[helper.second_path_id].abandon_challenge();
        assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
        assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

        // Trigger:
        helper
            .manager
            .on_non_path_validation_probing_packet(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert!(!helper.manager[helper.second_path_id].is_validated());
        assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
    //= type=test
    //# When an endpoint has no validated path on which to send packets, it
    //# MAY discard connection state.

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
    //= type=test
    //# If an endpoint has no state about the last validated peer address, it
    //# MUST close the connection silently by discarding all connection
    //# state.

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
    //= type=test
    //# An endpoint MAY discard connection state if it does not have a
    //# validated path on which it can send packets; see Section 8.2
    //
    // If there is no last_known_validated_path after a on_timeout then return a
    // NoValidPath error
    fn silently_return_when_there_is_no_valid_path() {
        // Setup:
        let now = NoopClock {}.get_time();
        let expiration = Duration::from_millis(1000);
        let challenge = challenge::Challenge::new(expiration, [0; 8]);
        let mut first_path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[1]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        first_path.set_challenge(challenge);
        let mut manager = manager(first_path, None);
        let first_path_id = Id(0);

        assert!(!manager[first_path_id].is_validated());
        assert!(manager[first_path_id].is_challenge_pending());
        assert_eq!(manager.last_known_validated_path, None);

        // Trigger:
        // send challenge and arm abandon timer
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        manager[first_path_id].on_transmit(&mut context);
        let res = manager.on_timeout(now + expiration + Duration::from_millis(100));

        // Expectation:
        assert!(!manager[first_path_id].is_challenge_pending());
        assert_eq!(res.unwrap_err(), connection::Error::NoValidPath);
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
    //= type=test
    //# After changing the address to which it sends non-probing packets, an
    //# endpoint can abandon any path validation for other addresses.
    //
    // A non-probing (path validation probing) packet will cause the path to become an active
    // path but the path is still not validated.
    fn dont_abandon_path_challenge_if_new_path_is_not_validated() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(!helper.manager[helper.first_path_id].is_validated());
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());

        assert!(!helper.manager[helper.second_path_id].is_validated());
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());
        assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

        // Trigger:
        helper
            .manager
            .on_non_path_validation_probing_packet(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert!(!helper.manager[helper.second_path_id].is_validated());
        assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());
    }

    #[test]
    fn abandon_path_challenges_if_new_path_is_validated() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());
        assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

        // simulate receiving a handshake packet to force path validation
        helper.manager[helper.second_path_id].on_handshake_packet();
        assert!(helper.manager[helper.second_path_id].is_validated());

        // Trigger:
        helper
            .manager
            .on_non_path_validation_probing_packet(
                helper.second_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher,
            )
            .unwrap();

        // Expectation:
        assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
        assert!(!helper.manager[helper.first_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
    }

    #[test]
    fn abandon_all_path_challenges() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        assert!(helper.manager[helper.zero_path_id].is_challenge_pending());
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());

        // Trigger:
        helper.manager.abandon_all_path_challenges();

        // Expectation:
        assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.first_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
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
            DEFAULT_MAX_MTU,
        );
        let mut manager = manager(first_path, None);

        // verify we have one path
        assert!(manager.path(&SocketAddress::default()).is_some());
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
                DEFAULT_MAX_MTU,
            )
            .unwrap();

        // Expectation:
        assert_eq!(path_id.0, 1);
        assert!(!unblocked);
        assert!(manager.path(&new_addr).is_some());
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
            DEFAULT_MAX_MTU,
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
            DEFAULT_MAX_MTU,
        );

        // Expectation:
        assert!(on_datagram_result.is_err());
        assert!(!manager.path(&new_addr).is_some());
        assert_eq!(manager.paths.len(), 1);
    }

    #[test]
    fn limit_number_of_connection_migrations() {
        // Setup:
        let first_path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[1]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        let mut manager = manager(first_path, None);
        let mut total_paths = 1;

        for i in 1..std::u8::MAX {
            let new_addr: SocketAddr = format!("127.0.0.1:{}", i).parse().unwrap();
            let new_addr = SocketAddress::from(new_addr);
            let now = NoopClock {}.get_time();
            let datagram = DatagramInfo {
                timestamp: now,
                remote_address: new_addr,
                payload_len: 0,
                ecn: ExplicitCongestionNotification::default(),
                destination_connection_id: connection::LocalId::TEST_ID,
            };

            let res = manager.handle_connection_migration(
                &datagram,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
                DEFAULT_MAX_MTU,
            );
            match res {
                Ok(_) => total_paths += 1,
                Err(_) => break,
            }
        }
        assert_eq!(total_paths, MAX_ALLOWED_PATHS);
    }

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
            DEFAULT_MAX_MTU,
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
                DEFAULT_MAX_MTU,
            )
            .unwrap();

        // verify we have two paths
        assert!(manager.path(&new_addr).is_some());
        assert_eq!(manager.paths.len(), 2);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //= type=test
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.
        assert!(manager[Id(1)].is_challenge_pending());

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
        manager[Id(1)].on_path_response(&expected_data);
        assert!(manager[Id(1)].is_validated());
    }

    #[test]
    // Abandon timer should use max PTO of active and new path(new path uses kInitialRtt)
    // Setup 1:
    // - create manager with path
    // - create datagram for packet on second path
    //
    // Trigger 1:
    // - call handle_connection_migration with packet for second path
    //
    // Expectation 1:
    // - assert that new path uses max_ack_delay from the active path
    fn connection_migration_use_max_ack_delay_from_active_path() {
        // Setup 1:
        let first_path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[1]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
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

        // Trigger 1:
        let (second_path_id, _unblocked) = manager
            .handle_connection_migration(
                &datagram,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
                DEFAULT_MAX_MTU,
            )
            .unwrap();
        let first_path_id = Id(0);

        // Expectation 1:
        // inherit max_ack_delay from the active path
        assert_eq!(manager.active, first_path_id.0);
        assert_eq!(
            &manager[first_path_id].rtt_estimator.max_ack_delay(),
            &manager[second_path_id].rtt_estimator.max_ack_delay()
        );
    }

    #[test]
    // Abandon timer should use max PTO of active and new path(new path uses kInitialRtt)
    // Setup 1:
    // - create manager with path
    // - create datagram for packet on second path
    // - call handle_connection_migration with packet for second path
    //
    // Trigger 1:
    // - modify rtt for fist path to detect difference in PTO
    //
    // Expectation 1:
    // - veify PTO of second path > PTO of first path
    //
    // Setup 2:
    // - call on_transmit for second path to send challenge and arm abandon timer
    //
    // Trigger 2:
    // - call second_path.on_timeout with abandon_time - 10ms
    //
    // Expectation 2:
    // - verify challenge is NOT abandoned
    //
    // Trigger 3:
    // - call second_path.on_timeout with abandon_time + 10ms
    //
    // Expectation 3:
    // - verify challenge is abandoned
    fn connection_migration_new_path_abandon_timer() {
        // Setup 1:
        let first_path = Path::new(
            SocketAddress::default(),
            connection::PeerId::try_from_bytes(&[1]).unwrap(),
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
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

        let (second_path_id, _unblocked) = manager
            .handle_connection_migration(
                &datagram,
                &mut unlimited::Endpoint::default(),
                &mut random::testing::Generator(123),
                DEFAULT_MAX_MTU,
            )
            .unwrap();
        let first_path_id = Id(0);

        // Trigger 1:
        // modify rtt for first path so we can detect differences
        manager[first_path_id].rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        // Expectation 1:
        // verify the pto_period of the first path is less than the second path
        let first_path_pto = manager[first_path_id].pto_period(PacketNumberSpace::ApplicationData);
        let second_path_pto =
            manager[second_path_id].pto_period(PacketNumberSpace::ApplicationData);

        assert_eq!(first_path_pto, Duration::from_millis(330));
        assert_eq!(second_path_pto, Duration::from_millis(1_029));
        assert!(second_path_pto > first_path_pto);

        // Setup 2:
        // send challenge and arm abandon timer
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        manager[second_path_id].on_transmit(&mut context);

        // Trigger 2:
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.4
        //= type=test
        //# Endpoints SHOULD abandon path validation based on a timer.
        //
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#8.2.4
        //= type=test
        //# When
        //# setting this timer, implementations are cautioned that the new path
        //# could have a longer round-trip time than the original. A value of
        //# three times the larger of the current Probe Timeout (PTO) or the PTO
        //# for the new path (that is, using kInitialRtt as defined in
        //# [QUIC-RECOVERY]) is RECOMMENDED.
        // abandon_duration should use max pto_period: second path
        let abandon_time = now + (second_path_pto * 3);
        manager[second_path_id].on_timeout(abandon_time - Duration::from_millis(10));

        // Expectation 2:
        assert!(manager[second_path_id].is_challenge_pending());

        // Trigger 3:
        manager[second_path_id].on_timeout(abandon_time + Duration::from_millis(10));
        // Expectation 3:
        assert!(!manager[second_path_id].is_challenge_pending());
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
            DEFAULT_MAX_MTU,
        );
        let mut manager = manager(first_path, None);

        let id_2 = connection::PeerId::try_from_bytes(b"id02").unwrap();
        assert!(manager
            .on_new_connection_id(&id_2, 1, 1, &TEST_TOKEN_1)
            .is_ok());

        assert_eq!(id_2, manager.paths[0].peer_connection_id);
    }

    #[test]
    fn amplification_limited_true_if_all_paths_amplificaiton_limited() {
        // Setup:
        let helper = helper_manager_with_paths_base(true, false);
        let zp = &helper.manager[helper.zero_path_id];
        assert!(zp.at_amplification_limit());
        let fp = &helper.manager[helper.first_path_id];
        assert!(fp.at_amplification_limit());
        let sp = &helper.manager[helper.second_path_id];
        assert!(sp.at_amplification_limit());

        // Expectation:
        assert!(helper.manager.is_amplification_limited());
    }

    #[test]
    fn amplification_limited_false_if_any_paths_amplificaiton_limited() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        let fp = &helper.manager[helper.first_path_id];
        assert!(fp.at_amplification_limit());
        let sp = &mut helper.manager[helper.second_path_id];
        sp.on_bytes_received(1200);
        assert!(!sp.at_amplification_limit());

        // Expectation:
        assert!(!helper.manager.is_amplification_limited());
    }

    #[test]
    fn can_transmit_false_if_no_path_can_transmit() {
        // Setup:
        let helper = helper_manager_with_paths_base(true, false);

        let zp = &helper.manager[helper.zero_path_id];
        assert!(!transmission::Interest::None.can_transmit(zp.transmission_constraint()));

        let interest = transmission::Interest::Forced;
        let fp = &helper.manager[helper.first_path_id];
        assert!(!interest.can_transmit(fp.transmission_constraint()));
        let sp = &helper.manager[helper.second_path_id];
        assert!(!interest.can_transmit(sp.transmission_constraint()));

        // Expectation:
        assert!(!helper.manager.can_transmit(interest));
    }

    #[test]
    fn can_transmit_true_if_any_path_can_transmit() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        let interest = transmission::Interest::Forced;
        let fp = &helper.manager[helper.first_path_id];
        assert!(!interest.can_transmit(fp.transmission_constraint()));

        let sp = &mut helper.manager[helper.second_path_id];
        sp.on_bytes_received(1200);
        assert!(interest.can_transmit(sp.transmission_constraint()));

        // Expectation:
        assert!(helper.manager.can_transmit(interest));
    }

    #[test]
    // Return all paths that are pending challenge or response.
    fn pending_paths_should_return_paths_pending_validation() {
        // Setup:
        let mut helper = helper_manager_with_paths();
        let third_path_id = Id(3);
        let third_conn_id = connection::PeerId::try_from_bytes(&[3]).unwrap();
        let mut third_path = Path::new(
            SocketAddress::default(),
            third_conn_id,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        let expected_response_data = [0; 8];
        third_path.on_path_challenge(&expected_response_data);
        helper.manager.paths.push(third_path);

        // not pending challenge or response
        helper.manager[helper.zero_path_id].abandon_challenge();
        assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.zero_path_id].is_response_pending());

        // pending challenge
        assert!(helper.manager[helper.first_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.first_path_id].is_response_pending());
        assert!(helper.manager[helper.second_path_id].is_challenge_pending());
        assert!(!helper.manager[helper.second_path_id].is_response_pending());

        // pending response
        assert!(!helper.manager[third_path_id].is_challenge_pending());
        assert!(helper.manager[third_path_id].is_response_pending());

        let mut pending_paths = helper.manager.paths_pending_validation();

        // inclusive range from 1 to 3
        for i in 1..=3 {
            // Trigger:
            let next = pending_paths.next_path();

            // Expectation:
            let (path_id, _path_manager) = next.unwrap();
            assert_eq!(path_id, Id(i));
        }

        // Trigger:
        let next = pending_paths.next_path();

        // Expectation:
        assert!(next.is_none());
    }

    fn helper_manager_with_paths_base(
        register_second_conn_id: bool,
        validate_path_zero: bool,
    ) -> Helper {
        let zero_conn_id = connection::PeerId::try_from_bytes(&[0]).unwrap();
        let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
        let second_conn_id = connection::PeerId::try_from_bytes(&[2]).unwrap();
        let zero_path_id = Id(0);
        let first_path_id = Id(1);
        let second_path_id = Id(2);
        let local_conn_id = connection::LocalId::TEST_ID;
        let mut zero_path = Path::new(
            SocketAddress::default(),
            zero_conn_id,
            local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        if validate_path_zero {
            // simulate receiving a handshake packet to force path validation
            zero_path.on_handshake_packet();
        }
        assert!(!zero_path.is_challenge_pending());

        let now = NoopClock {}.get_time();
        let challenge_expiration = Duration::from_millis(10_000);
        let expected_data = [0; 8];
        let challenge = challenge::Challenge::new(challenge_expiration, expected_data);

        let mut first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        first_path.set_challenge(challenge);

        // Create a challenge that will expire in 100ms
        let expected_data = [1; 8];
        let challenge = challenge::Challenge::new(challenge_expiration, expected_data);
        let mut second_path = Path::new(
            SocketAddress::default(),
            second_conn_id,
            local_conn_id,
            RttEstimator::new(Duration::from_millis(30)),
            Default::default(),
            false,
            DEFAULT_MAX_MTU,
        );
        second_path.set_challenge(challenge);

        let mut random_generator = random::testing::Generator(123);
        let mut peer_id_registry =
            ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
                .create_peer_id_registry(
                    InternalConnectionIdGenerator::new().generate_id(),
                    zero_path.peer_connection_id,
                    None,
                );
        assert!(peer_id_registry
            .on_new_connection_id(&first_conn_id, 1, 0, &TEST_TOKEN_1)
            .is_ok());

        if register_second_conn_id {
            assert!(peer_id_registry
                .on_new_connection_id(&second_conn_id, 2, 0, &TEST_TOKEN_2)
                .is_ok());
        }

        let mut manager = Manager::new(zero_path, peer_id_registry);
        assert!(manager.peer_id_registry.is_active(&first_conn_id));
        manager.paths.push(first_path);
        manager.paths.push(second_path);
        assert_eq!(manager.paths.len(), 3);

        // update active path to first_path
        assert_eq!(manager.active, zero_path_id.0);
        if validate_path_zero {
            assert!(manager.active_path().is_validated());
        }

        assert!(manager
            .update_active_path(
                first_path_id,
                &mut random::testing::Generator(123),
                &mut Publisher
            )
            .is_ok());
        if validate_path_zero {
            assert!(manager[zero_path_id].is_challenge_pending());
        }

        assert!(manager.peer_id_registry.consume_new_id().is_some());

        // assert first_path is active and last_known_validated_path
        assert!(manager.peer_id_registry.is_active(&first_conn_id));
        assert_eq!(manager.active, first_path_id.0);

        if validate_path_zero {
            assert_eq!(manager.last_known_validated_path, Some(zero_path_id.0));
        } else {
            assert_eq!(manager.last_known_validated_path, None);
        }

        Helper {
            now,
            expected_data,
            challenge_expiration,
            zero_path_id,
            first_path_id,
            second_path_id,
            manager,
        }
    }

    fn helper_manager_with_paths() -> Helper {
        helper_manager_with_paths_base(true, true)
    }

    struct Helper {
        pub now: Timestamp,
        pub expected_data: challenge::Data,
        pub challenge_expiration: Duration,
        pub zero_path_id: Id,
        pub first_path_id: Id,
        pub second_path_id: Id,
        pub manager: Manager<unlimited::Endpoint>,
    }
}
