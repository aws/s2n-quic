// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Manager implementation

use crate::{
    connection::PeerIdRegistry,
    endpoint,
    path::{challenge, Path},
    transmission,
};
use s2n_quic_core::{
    ack, connection,
    event::{self, IntoEvent},
    frame,
    frame::path_validation,
    inet::DatagramInfo,
    packet::number::PacketNumberSpace,
    path::{Handle as _, MaxMtu},
    random,
    recovery::{
        congestion_controller::{self, Endpoint as _},
        RttEstimator,
    },
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
pub struct Manager<Config: endpoint::Config> {
    /// Path array
    paths: SmallVec<[Path<Config>; MAX_ALLOWED_PATHS]>,

    /// Registry of `connection::PeerId`s
    peer_id_registry: PeerIdRegistry,

    /// Index to the active path
    active: u8,

    /// Index of last known validated path
    last_known_validated_path: Option<u8>,
}

impl<Config: endpoint::Config> Manager<Config> {
    pub fn new(initial_path: Path<Config>, peer_id_registry: PeerIdRegistry) -> Self {
        Manager {
            paths: SmallVec::from_elem(initial_path, 1),
            peer_id_registry,
            active: 0,
            last_known_validated_path: None,
        }
    }

    /// Update the active path
    fn update_active_path<Rnd: random::Generator, Pub: event::ConnectionPublisher>(
        &mut self,
        new_path_id: Id,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(new_path_id != Id(self.active));

        let prev_path_id = self.active_path_id();

        let mut peer_connection_id = self[new_path_id].peer_connection_id;

        // The path's connection id might have retired since we last used it. Check if it is still
        // active, otherwise try and consume a new connection id.
        if !self.peer_id_registry.is_active(&peer_connection_id) {
            // TODO https://github.com/awslabs/s2n-quic/issues/669
            // If there are no new connection ids the peer is responsible for
            // providing additional connection ids to continue.
            //
            // Insufficient connection ids should not cause the connection to close.
            // Investigate api after this is used.
            peer_connection_id = self
                .peer_id_registry
                .consume_new_id_for_existing_path(new_path_id, peer_connection_id, publisher)
                .ok_or(
                    // TODO: add an event if active path update fails due to insufficient ids
                    transport::Error::INTERNAL_ERROR,
                )?;
        };
        self[new_path_id].peer_connection_id = peer_connection_id;

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

        self.active = new_path_id.as_u8();

        let prev_path = &self[prev_path_id];
        let new_path = &self[new_path_id];
        publisher.on_active_path_updated(event::builder::ActivePathUpdated {
            previous: path_event!(prev_path, prev_path_id),
            active: path_event!(new_path, new_path_id),
        });

        // Restart ECN validation to check that the path still supports ECN
        self.active_path_mut().ecn_controller.restart();

        Ok(())
    }

    /// Return the active path
    #[inline]
    pub fn active_path(&self) -> &Path<Config> {
        &self.paths[self.active as usize]
    }

    /// Return a mutable reference to the active path
    #[inline]
    pub fn active_path_mut(&mut self) -> &mut Path<Config> {
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
    pub fn path(&self, handle: &Config::PathHandle) -> Option<(Id, &Path<Config>)> {
        self.paths
            .iter()
            .enumerate()
            .find(|(_id, path)| path.handle.eq(handle))
            .map(|(id, path)| (Id(id as u8), path))
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    #[inline]
    pub fn path_mut(&mut self, handle: &Config::PathHandle) -> Option<(Id, &mut Path<Config>)> {
        self.paths
            .iter_mut()
            .enumerate()
            .find(|(_id, path)| path.handle.eq(handle))
            .map(|(id, path)| (Id(id as u8), path))
    }

    /// Returns an iterator over all paths pending path_challenge or path_response
    /// transmission.
    pub fn paths_pending_validation(&mut self) -> PathsPendingValidation<Config> {
        PathsPendingValidation::new(self)
    }

    /// Called when a datagram is received on a connection
    /// Upon success, returns a `(Id, bool)` containing the path ID and a boolean that is
    /// true if the path had been amplification limited prior to receiving the datagram
    /// and is now no longer amplification limited.
    #[allow(unused_variables, clippy::too_many_arguments)]
    pub fn on_datagram_received<Rnd: random::Generator, Pub: event::ConnectionPublisher>(
        &mut self,
        path_handle: &Config::PathHandle,
        datagram: &DatagramInfo,
        limits: &connection::Limits,
        handshake_confirmed: bool,
        congestion_controller_endpoint: &mut Config::CongestionControllerEndpoint,
        random_generator: &mut Rnd,
        max_mtu: MaxMtu,
        publisher: &mut Pub,
    ) -> Result<(Id, bool), transport::Error> {
        if let Some((id, path)) = self.path_mut(path_handle) {
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
            path_handle,
            datagram,
            congestion_controller_endpoint,
            random_generator,
            max_mtu,
            publisher,
        )
    }

    #[allow(unreachable_code)]
    #[allow(unused_variables)]
    fn handle_connection_migration<Rnd: random::Generator, Pub: event::ConnectionPublisher>(
        &mut self,
        path_handle: &Config::PathHandle,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut Config::CongestionControllerEndpoint,
        random_generator: &mut Rnd,
        max_mtu: MaxMtu,
        publisher: &mut Pub,
    ) -> Result<(Id, bool), transport::Error> {
        // Since we are not currently supporting connection migration (whether it was deliberate or
        // not), we will error our at this point to avoid re-using a peer connection ID.
        // TODO: This would be better handled as a stateless reset so the peer can terminate the
        //       connection immediately. https://github.com/awslabs/s2n-quic/issues/317
        // We only enable connection migration for testing
        #[cfg(not(any(feature = "connection-migration", feature = "testing", test)))]
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
        let remote_address = path_handle.remote_address();
        let path_info = congestion_controller::PathInfo::new(&remote_address);
        let cc = congestion_controller_endpoint.new_congestion_controller(path_info);

        let peer_connection_id = {
            if self.active_path().local_connection_id != datagram.destination_connection_id {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.5
                //# Similarly, an endpoint MUST NOT reuse a connection ID when sending to
                //# more than one destination address.

                // Peer has intentionally tried to migrate to this new path because they changed
                // their destination_connection_id, so we will change our destination_connection_id as well.
                self.peer_id_registry
                    .consume_new_id_for_new_path()
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
            *path_handle,
            peer_connection_id,
            datagram.destination_connection_id,
            rtt,
            cc,
            true,
            max_mtu,
        );

        let unblocked = path.on_bytes_received(datagram.payload_len);

        let active_path = self.active_path();
        let active_path_id = self.active_path_id();
        publisher.on_path_created(event::builder::PathCreated {
            active: path_event!(active_path, active_path_id),
            new: path_event!(path, new_path_id),
        });

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
        path_id: Id,
        challenge: &frame::path_challenge::PathChallenge,
    ) {
        self[path_id].on_path_challenge(challenge.data);
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

    /// Process a packet and update internal state.
    ///
    /// Check if the packet is a non-probing (path validation) packet and attempt to
    /// update the active path for the connection.
    pub fn on_processed_packet<Rnd: random::Generator, Pub: event::ConnectionPublisher>(
        &mut self,
        path_id: Id,
        path_validation_probing: path_validation::Probe,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.2
        //# An endpoint can migrate a connection to a new local address by
        //# sending packets containing non-probing frames from that address.
        if !path_validation_probing.is_probing() && self.active_path_id() != path_id {
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
    pub fn on_new_connection_id<Pub: event::ConnectionPublisher>(
        &mut self,
        connection_id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: &stateless_reset::Token,
        publisher: &mut Pub,
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
            self.active_path_mut().peer_connection_id = self
                .peer_id_registry
                .consume_new_id_for_existing_path(
                    self.active_path_id(),
                    active_path_connection_id,
                    publisher,
                )
                .expect(
                    "since we are only checking the active path and new id was delivered \
                    via the new_connection_id frames, there will always be a new id available \
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

impl<Config: endpoint::Config> timer::Provider for Manager<Config> {
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
pub struct PathsPendingValidation<'a, Config: endpoint::Config> {
    index: u8,
    path_manager: &'a mut Manager<Config>,
}

impl<'a, Config: endpoint::Config> PathsPendingValidation<'a, Config> {
    pub fn new(path_manager: &'a mut Manager<Config>) -> Self {
        Self {
            index: 0,
            path_manager,
        }
    }

    #[inline]
    pub fn next_path(&mut self) -> Option<(Id, &mut Manager<Config>)> {
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

impl<Config: endpoint::Config> transmission::interest::Provider for Manager<Config> {
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

impl<Config: endpoint::Config> core::ops::Index<Id> for Manager<Config> {
    type Output = Path<Config>;

    #[inline]
    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.0 as usize]
    }
}

impl<Config: endpoint::Config> core::ops::IndexMut<Id> for Manager<Config> {
    #[inline]
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.0 as usize]
    }
}

impl event::IntoEvent<u64> for Id {
    #[inline]
    fn into_event(self) -> u64 {
        self.0 as u64
    }
}

macro_rules! path_event {
    ($path:ident, $path_id:ident) => {{
        event::builder::Path {
            local_addr: $path.local_address().into_event(),
            local_cid: $path.local_connection_id.into_event(),
            remote_addr: $path.remote_address().into_event(),
            remote_cid: $path.peer_connection_id.into_event(),
            id: $path_id.into_event(),
        }
    }};
}
pub(crate) use path_event;

#[cfg(test)]
mod tests;
