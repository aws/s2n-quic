// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the Manager implementation

use crate::{
    connection::PeerIdRegistry,
    endpoint, path,
    path::{challenge, Path},
    transmission,
};
use core::time::Duration;
use s2n_quic_core::{
    ack,
    connection::{self, PeerId},
    event::{
        self,
        builder::{DatagramDropReason, MtuUpdatedCause},
        IntoEvent,
    },
    frame,
    frame::path_validation,
    inet::DatagramInfo,
    packet::number::PacketNumberSpace,
    path::{
        migration::{self, Validator as _},
        mtu, Handle as _, Id, MaxMtu,
    },
    random,
    recovery::congestion_controller::{self, Endpoint as _},
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
    pub(crate) peer_id_registry: PeerIdRegistry,

    /// Index to the active path
    active: u8,

    /// Index of last known active and validated path.
    ///
    /// The Path must be validated and also active at some some point to
    /// be set as the last_known_active_validated_path.
    last_known_active_validated_path: Option<u8>,

    /// The current index of a path that is pending packet protection authentication
    ///
    /// This field is used to annotate a new path that is pending packet authentication.
    /// If packet authentication fails then this path index will get reused instead of
    /// appending another to the list. This is used to prevent an off-path attacker from
    /// creating new paths with garbage data and preventing the peer to migrate paths.
    ///
    /// Note that it doesn't prevent an on-path attacker from observing/forwarding
    /// authenticated packets from bogus addresses. Because of the current hard limit
    /// of `MAX_ALLOWED_PATHS`, this will prevent the peer from migrating, if it needs to.
    /// The `paths` data structure will need to be enhanced to include garbage collection
    /// of old paths to overcome this limitation.
    pending_packet_authentication: Option<u8>,
}

impl<Config: endpoint::Config> Manager<Config> {
    pub fn new(initial_path: Path<Config>, peer_id_registry: PeerIdRegistry) -> Self {
        let mut manager = Manager {
            paths: SmallVec::from_elem(initial_path, 1),
            peer_id_registry,
            active: 0,
            last_known_active_validated_path: None,
            pending_packet_authentication: None,
        };
        manager.paths[0].activated = true;
        manager.paths[0].is_active = true;
        manager
    }

    /// Update the active path
    fn update_active_path<Pub: event::ConnectionPublisher>(
        &mut self,
        new_path_id: Id,
        random_generator: &mut dyn random::Generator,
        publisher: &mut Pub,
    ) -> Result<AmplificationOutcome, transport::Error> {
        debug_assert!(new_path_id != path_id(self.active));

        let prev_path_id = self.active_path_id();

        let mut peer_connection_id = self[new_path_id].peer_connection_id;

        // The path's connection id might have retired since we last used it. Check if it is still
        // active, otherwise try and consume a new connection id.
        if !self.peer_id_registry.is_active(&peer_connection_id) {
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
            self.last_known_active_validated_path = Some(self.active);
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.3
        //# In response to an apparent migration, endpoints MUST validate the
        //# previously active path using a PATH_CHALLENGE frame.
        //
        // TODO: https://github.com/aws/s2n-quic/issues/711
        // The usage of 'apparent' is vague and its not clear if the previous path should
        // always be validated or only if the new active path is not validated.
        if !self.active_path().is_challenge_pending() {
            self.set_challenge(self.active_path_id(), random_generator);
        }

        let amplification_outcome = self.activate_path(publisher, prev_path_id, new_path_id);

        // Restart ECN validation to check that the path still supports ECN
        let path = self.active_path_mut();
        path.ecn_controller
            .restart(path_event!(path, new_path_id), publisher);
        Ok(amplification_outcome)
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
        path_id(self.active)
    }

    pub fn check_active_path_is_synced(&self) {
        if cfg!(debug_assertions) {
            for (idx, path) in self.paths.iter().enumerate() {
                assert_eq!(path.is_active, (self.active == idx as u8));
            }
        }
    }

    fn activate_path<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
        prev_path_id: Id,
        new_path_id: Id,
    ) -> AmplificationOutcome {
        self.check_active_path_is_synced();
        self.active = new_path_id.as_u8();
        self[prev_path_id].is_active = false;
        self[new_path_id].is_active = true;
        self[new_path_id].on_activated();
        let amplification_outcome = if self[prev_path_id].at_amplification_limit()
            && !self[new_path_id].at_amplification_limit()
        {
            AmplificationOutcome::ActivePathUnblocked
        } else {
            AmplificationOutcome::Unchanged
        };
        self.check_active_path_is_synced();

        let prev_path = &self[prev_path_id];
        let new_path = &self[new_path_id];
        publisher.on_active_path_updated(event::builder::ActivePathUpdated {
            previous: path_event!(prev_path, prev_path_id),
            active: path_event!(new_path, new_path_id),
        });

        amplification_outcome
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
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
            .find(|(_id, path)| Path::eq_by_handle(path, handle))
            .map(|(id, path)| (path_id(id as u8), path))
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    #[inline]
    pub fn path_mut(&mut self, handle: &Config::PathHandle) -> Option<(Id, &mut Path<Config>)> {
        self.paths
            .iter_mut()
            .enumerate()
            .find(|(_id, path)| Path::eq_by_handle(path, handle))
            .map(|(id, path)| (path_id(id as u8), path))
    }

    /// Returns an iterator over all paths pending path_challenge or path_response
    /// transmission.
    pub fn paths_pending_validation(&mut self) -> PathsPendingValidation<Config> {
        PathsPendingValidation::new(self)
    }

    /// Called when a datagram is received on a connection
    /// Upon success, returns a `(Id, AmplificationOutcome)` containing the path ID and an
    /// `AmplificationOutcome` value that indicates if the path had been amplification limited
    /// prior to receiving the datagram and is now no longer amplification limited.
    ///
    /// This function is called prior to packet authentication. If possible add business
    /// logic to [`Self::on_processed_packet`], which is called after the packet has been
    /// authenticated.
    #[allow(clippy::too_many_arguments)]
    pub fn on_datagram_received<Pub: event::ConnectionPublisher>(
        &mut self,
        path_handle: &Config::PathHandle,
        datagram: &DatagramInfo,
        handshake_confirmed: bool,
        congestion_controller_endpoint: &mut Config::CongestionControllerEndpoint,
        migration_validator: &mut Config::PathMigrationValidator,
        mtu_config: mtu::Config,
        initial_rtt: Duration,
        publisher: &mut Pub,
    ) -> Result<(Id, AmplificationOutcome), DatagramDropReason> {
        let valid_initial_received = self.valid_initial_received();

        if let Some((id, path)) = self.path_mut(path_handle) {
            let source_cid_changed = datagram.source_connection_id.map_or(false, |scid| {
                scid != path.peer_connection_id && valid_initial_received
            });

            if source_cid_changed {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
                //# Once a client has received a valid Initial packet from the server, it MUST
                //# discard any subsequent packet it receives on that connection with a
                //# different Source Connection ID.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
                //# Any further changes to the Destination Connection ID are only
                //# permitted if the values are taken from NEW_CONNECTION_ID frames; if
                //# subsequent Initial packets include a different Source Connection ID,
                //# they MUST be discarded.

                return Err(DatagramDropReason::InvalidSourceConnectionId);
            }

            // update the address if it was resolved
            path.handle.maybe_update(path_handle);

            let amplification_outcome = path.on_bytes_received(datagram.payload_len);
            return Ok((id, amplification_outcome));
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9
        //# If a client receives packets from an unknown server address,
        //# the client MUST discard these packets.
        if Config::ENDPOINT_TYPE.is_client() {
            return Err(DatagramDropReason::UnknownServerAddress);
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9
        //# The design of QUIC relies on endpoints retaining a stable address
        //# for the duration of the handshake.  An endpoint MUST NOT initiate
        //# connection migration before the handshake is confirmed, as defined
        //# in section 4.1.2 of [QUIC-TLS].
        if !handshake_confirmed {
            return Err(DatagramDropReason::ConnectionMigrationDuringHandshake);
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9
        //# If the peer
        //# violates this requirement, the endpoint MUST either drop the incoming
        //# packets on that path without generating a Stateless Reset or proceed
        //# with path validation and allow the peer to migrate.  Generating a
        //# Stateless Reset or closing the connection would allow third parties
        //# in the network to cause connections to close by spoofing or otherwise
        //# manipulating observed traffic.

        self.handle_connection_migration(
            path_handle,
            datagram,
            congestion_controller_endpoint,
            migration_validator,
            mtu_config,
            initial_rtt,
            publisher,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_connection_migration<Pub: event::ConnectionPublisher>(
        &mut self,
        path_handle: &Config::PathHandle,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut Config::CongestionControllerEndpoint,
        migration_validator: &mut Config::PathMigrationValidator,
        mtu_config: mtu::Config,
        initial_rtt: Duration,
        publisher: &mut Pub,
    ) -> Result<(Id, AmplificationOutcome), DatagramDropReason> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-9
        //# Clients are responsible for initiating all migrations.
        debug_assert!(Config::ENDPOINT_TYPE.is_server());

        let remote_address = path_handle.remote_address();
        let local_address = path_handle.local_address();
        let active_local_addr = self.active_path().local_address();
        let active_remote_addr = self.active_path().remote_address();

        // TODO set alpn if available
        let attempt: migration::Attempt = migration::AttemptBuilder {
            active_path: event::builder::Path {
                local_addr: active_local_addr.into_event(),
                local_cid: self.active_path().local_connection_id.into_event(),
                remote_addr: active_remote_addr.into_event(),
                remote_cid: self.active_path().peer_connection_id.into_event(),
                id: self.active_path_id().into_event(),
                is_active: true,
            }
            .into_event(),
            packet: migration::PacketInfoBuilder {
                remote_address: &remote_address,
                local_address: &local_address,
            }
            .into(),
        }
        .into();

        match migration_validator.on_migration_attempt(&attempt) {
            migration::Outcome::Allow => {
                // no-op: allow the migration to continue
            }
            migration::Outcome::Deny(reason) => {
                publisher.on_connection_migration_denied(reason.into_event());
                return Err(DatagramDropReason::RejectedConnectionMigration);
            }
            _ => {
                unimplemented!("unimplemented migration outcome");
            }
        }

        // Determine which index will be used for the newly created path
        //
        // If a previously allocated path failed to contain an authenticated packet, we
        // use that index instead of pushing on to the end.
        let new_path_idx = if let Some(idx) = self.pending_packet_authentication {
            idx as _
        } else {
            let idx = self.paths.len();
            self.pending_packet_authentication = Some(idx as _);
            idx
        };

        // TODO: Support deletion of old paths: https://github.com/aws/s2n-quic/issues/741
        // The current path manager implementation does not delete or reuse indices
        // in the path array. This can result in an unbounded number of paths. To prevent
        // this we limit the max number of paths per connection.
        if new_path_idx >= MAX_ALLOWED_PATHS {
            return Err(DatagramDropReason::PathLimitExceeded);
        }
        let new_path_id = path_id(new_path_idx as u8);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.4
        //= type=TODO
        //# Because port-only changes are commonly the
        //# result of NAT rebinding or other middlebox activity, the endpoint MAY
        //# instead retain its congestion control state and round-trip estimate
        //# in those cases instead of reverting to initial values.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.4
        //# On confirming a peer's ownership of its new address, an endpoint MUST
        //# immediately reset the congestion controller and round-trip time
        //# estimator for the new path to initial values (see Appendices A.3 and
        //# B.3 of [QUIC-RECOVERY]) unless the only change in the peer's address
        //# is its port number.
        // Since we maintain a separate congestion controller and round-trip time
        // estimator for the new path, and they are initialized with initial values,
        // we do not need to reset congestion controller and round-trip time estimator
        // again on confirming the peer's ownership of its new address.
        let rtt = self.active_path().rtt_estimator.for_new_path(initial_rtt);
        let path_info =
            congestion_controller::PathInfo::new(mtu_config.initial_mtu, &remote_address);
        let cc = congestion_controller_endpoint.new_congestion_controller(path_info);

        let peer_connection_id = {
            if self.active_path().local_connection_id != datagram.destination_connection_id {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-9.5
                //# Similarly, an endpoint MUST NOT reuse a connection ID when sending to
                //# more than one destination address.

                // Peer has intentionally tried to migrate to this new path because they changed
                // their destination_connection_id, so we will change our destination_connection_id as well.
                self.peer_id_registry
                    .consume_new_id_for_new_path()
                    .ok_or(DatagramDropReason::InsufficientConnectionIds)?
            } else {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-9.5
                //# Due to network changes outside
                //# the control of its peer, an endpoint might receive packets from a new
                //# source address with the same Destination Connection ID field value,
                //# in which case it MAY continue to use the current connection ID with
                //# the new remote address while still sending from the same local
                //# address.
                self.active_path().peer_connection_id
            }
        };

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.1
        //# Until a peer's address is deemed valid, an endpoint limits
        //# the amount of data it sends to that address; see Section 8.
        //
        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
        //# An endpoint MAY send data to an unvalidated peer address, but it MUST
        //# protect against potential attacks as described in Sections 9.3.1 and
        //# 9.3.2.
        //
        // New paths for a Server endpoint start in AmplificationLimited state until they are validated.
        let mut path = Path::new(
            *path_handle,
            peer_connection_id,
            datagram.destination_connection_id,
            rtt,
            cc,
            true,
            mtu_config,
        );

        let amplification_outcome = path.on_bytes_received(datagram.payload_len);

        let active_path = self.active_path();
        let active_path_id = self.active_path_id();
        publisher.on_path_created(event::builder::PathCreated {
            active: path_event!(active_path, active_path_id),
            new: path_event!(path, new_path_id),
        });

        publisher.on_mtu_updated(event::builder::MtuUpdated {
            path_id: new_path_id.into_event(),
            mtu: path.mtu_controller.mtu() as u16,
            cause: MtuUpdatedCause::NewPath,
        });

        // create a new path
        if new_path_idx < self.paths.len() {
            self.paths[new_path_idx] = path;
        } else {
            self.paths.push(path);
        }

        Ok((new_path_id, amplification_outcome))
    }

    fn set_challenge(&mut self, path_id: Id, random_generator: &mut dyn random::Generator) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.1
        //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
        //# frame so that it can associate the peer's response with the
        //# corresponding PATH_CHALLENGE.
        let mut data: challenge::Data = [0; 8];
        random_generator.public_random_fill(&mut data);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.4
        //# Endpoints SHOULD abandon path validation based on a timer.
        //
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.4
        //# When
        //# setting this timer, implementations are cautioned that the new path
        //# could have a longer round-trip time than the original.  A value of
        //# three times the larger of the current PTO or the PTO for the new path
        //# (using kInitialRtt, as defined in [QUIC-RECOVERY]) is RECOMMENDED.
        let abandon_duration = self[path_id].pto_period(PacketNumberSpace::ApplicationData);
        let abandon_duration = 3 * abandon_duration.max(
            self.active_path()
                .pto_period(PacketNumberSpace::ApplicationData),
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9
        //# An endpoint MUST
        //# perform path validation (Section 8.2) if it detects any change to a
        //# peer's address, unless it has previously validated that address.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.6.3
        //# Servers SHOULD initiate path validation to the client's new address
        //# upon receiving a probe packet from a different address.
        let challenge = challenge::Challenge::new(abandon_duration, data);
        self[path_id].set_challenge(challenge);
    }

    /// Returns true if a valid initial packet has been received
    pub fn valid_initial_received(&self) -> bool {
        if Config::ENDPOINT_TYPE.is_server() {
            // Since the path manager is owned by a connection, and a connection can only exist
            // on the server if a valid initial has been received, we immediately return true
            true
        } else {
            // A QUIC client uses a randomly generated value as the Initial Connection Id
            // until it receives a packet from the Server. Upon receiving a Server packet,
            // the Client switches to using the new Destination Connection Id. The
            // PeerIdRegistry is expected to be empty until the first Server initial packet.
            !self.peer_id_registry.is_empty()
        }
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

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.3
    //# Path validation succeeds when a PATH_RESPONSE frame is received that
    //# contains the data that was sent in a previous PATH_CHALLENGE frame.
    //# A PATH_RESPONSE frame received on any network path validates the path
    //# on which the PATH_CHALLENGE was sent.
    #[inline]
    pub fn on_path_response<Pub: event::ConnectionPublisher>(
        &mut self,
        response: &frame::PathResponse,
        publisher: &mut Pub,
    ) -> AmplificationOutcome {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //# A PATH_RESPONSE frame MUST be sent on the network path where the
        //# PATH_CHALLENGE frame was received.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
        //# This requirement MUST NOT be enforced by the endpoint that initiates
        //# path validation, as that would enable an attack on migration; see
        //# Section 9.3.3.
        //
        // The 'attack on migration' refers to the following scenario:
        // If the packet forwarded by the off-attacker is received before the
        // genuine packet, the genuine packet will be discarded as a duplicate
        // and path validation will fail.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.3
        //# A PATH_RESPONSE frame received on any network path validates the path
        //# on which the PATH_CHALLENGE was sent.

        for (id, path) in self.paths.iter_mut().enumerate() {
            let was_amplification_limited = path.at_amplification_limit();
            if path.on_path_response(response.data) {
                let id = id as u64;
                publisher.on_path_challenge_updated(event::builder::PathChallengeUpdated {
                    path_challenge_status: event::builder::PathChallengeStatus::Validated,
                    path: path_event!(path, id),
                    challenge_data: path.challenge.challenge_data().into_event(),
                });
                // A path was validated so check if it becomes the new
                // last_known_active_validated_path
                if path.is_activated() {
                    self.last_known_active_validated_path = Some(id as u8);
                }
                // The path is now validated, so it is unblocked if it was
                // previously amplification limited
                debug_assert!(!path.at_amplification_limit());
                return match (was_amplification_limited, path.is_active()) {
                    (true, true) => AmplificationOutcome::ActivePathUnblocked,
                    (true, false) => AmplificationOutcome::InactivePathUnblocked,
                    _ => AmplificationOutcome::Unchanged,
                };
            }
        }
        AmplificationOutcome::Unchanged
    }

    /// Process a packet and update internal state.
    ///
    /// Check if the packet is a non-probing (path validation) packet and attempt to
    /// update the active path for the connection.
    ///
    /// Returns `Ok(true)` if the packet caused the active path to change from a path
    /// blocked by amplification limits to a path not blocked by amplification limits.
    pub fn on_processed_packet<Pub: event::ConnectionPublisher>(
        &mut self,
        path_id: Id,
        source_connection_id: Option<PeerId>,
        path_validation_probing: path_validation::Probe,
        random_generator: &mut dyn random::Generator,
        publisher: &mut Pub,
    ) -> Result<AmplificationOutcome, transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
        //# A client MUST change the Destination Connection ID it uses for
        //# sending packets in response to only the first received Initial or
        //# Retry packet.
        if !self.valid_initial_received() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
            //# Until a packet is received from the server, the client MUST
            //# use the same Destination Connection ID value on all packets in this
            //# connection.
            //
            // This is the first Server packet so start using the newly provided
            // connection id form the Server.
            assert!(Config::ENDPOINT_TYPE.is_client());
            if let Some(source_connection_id) = source_connection_id {
                self[path_id].peer_connection_id = source_connection_id;
                self.peer_id_registry
                    .register_initial_connection_id(source_connection_id);
            }
        }

        // Remove the temporary status after successfully processing a packet
        if self.pending_packet_authentication == Some(path_id.as_u8()) {
            self.pending_packet_authentication = None;

            // We can finally arm the challenge after authenticating the packet
            self.set_challenge(path_id, random_generator);
        }

        let mut amplification_outcome = AmplificationOutcome::Unchanged;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.2
        //# An endpoint can migrate a connection to a new local address by
        //# sending packets containing non-probing frames from that address.
        if !path_validation_probing.is_probing() && self.active_path_id() != path_id {
            amplification_outcome =
                self.update_active_path(path_id, random_generator, publisher)?;
            //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
            //# After changing the address to which it sends non-probing packets, an
            //# endpoint can abandon any path validation for other addresses.
            //
            // Abandon other path validations only if the active path is validated since an
            // attacker could block all path validation attempts simply by forwarding packets.
            if self.active_path().is_validated() {
                self.abandon_all_path_challenges(publisher);
            } else if !self.active_path().is_challenge_pending() {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
                //# If the recipient permits the migration, it MUST send subsequent
                //# packets to the new peer address and MUST initiate path validation
                //# (Section 8.2) to verify the peer's ownership of the address if
                //# validation is not already underway.
                self.set_challenge(self.active_path_id(), random_generator);
            }
        }
        Ok(amplification_outcome)
    }

    #[inline]
    fn abandon_all_path_challenges<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
    ) {
        for (idx, path) in self.paths.iter_mut().enumerate() {
            let path_id = idx as u64;
            path.abandon_challenge(publisher, path_id);
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
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
                .ok_or(transport::Error::PROTOCOL_VIOLATION.with_reason(
                    "A NEW_CONNECTION_ID frame was received that retired the active path's \
                    connection ID and no unused connection IDs remain to replace it",
                ))?
        }

        Ok(())
    }

    /// Called when the connection timer expired
    ///
    /// Returns `Ok(true)` if the timeout caused the active path to change from a path
    /// blocked by amplification limits to a path not blocked by amplification limits.
    /// This can occur if the active path was amplification limited and failed path validation.
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        random_generator: &mut dyn random::Generator,
        publisher: &mut Pub,
    ) -> Result<AmplificationOutcome, connection::Error> {
        for (id, path) in self.paths.iter_mut().enumerate() {
            path.on_timeout(timestamp, path_id(id as u8), random_generator, publisher);
        }

        let mut amplification_outcome = AmplificationOutcome::Unchanged;

        if self.active_path().failed_validation() {
            match self.last_known_active_validated_path {
                Some(last_known_active_validated_path) => {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.2
                    //# To protect the connection from failing due to such a spurious
                    //# migration, an endpoint MUST revert to using the last validated peer
                    //# address when validation of a new peer address fails.
                    let prev_path_id = path_id(self.active);
                    let new_path_id = path_id(last_known_active_validated_path);
                    amplification_outcome =
                        self.activate_path(publisher, prev_path_id, new_path_id);
                    self.last_known_active_validated_path = None;
                }
                None => {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-9
                    //# When an endpoint has no validated path on which to send packets, it
                    //# MAY discard connection state.

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-9
                    //= type=TODO
                    //= tracking-issue=713
                    //# An endpoint capable of connection
                    //# migration MAY wait for a new path to become available before
                    //# discarding connection state.

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.2
                    //# If an endpoint has no state about the last validated peer address, it
                    //# MUST close the connection silently by discarding all connection
                    //# state.

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-10
                    //# An endpoint MAY discard connection state if it does not have a
                    //# validated path on which it can send packets; see Section 8.2
                    return Err(connection::Error::no_valid_path());
                }
            }
        }

        Ok(amplification_outcome)
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

    /// Returns the maximum size the UDP payload can reach for any probe packet.
    #[inline]
    pub fn max_mtu(&self) -> MaxMtu {
        let value = self.active_path().max_mtu();

        // This value is the same for each path so just return the active value
        if cfg!(debug_assertions) {
            for path in self.paths.iter() {
                assert_eq!(value, path.max_mtu());
            }
        }

        value
    }

    #[cfg(test)]
    pub(crate) fn activate_path_for_test(&mut self, path_id: Id) {
        self[path_id].activated = true;
        self[path_id].is_active = true;
        self.active = path_id.as_u8();
    }
}

#[inline]
fn path_id(id: u8) -> path::Id {
    // Safety: The path::Manager is responsible for managing path ID and is thus
    //         responsible for using them safely
    unsafe { path::Id::new(id) }
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
                return Some((path_id(self.index - 1), self.path_manager));
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

impl<Config: endpoint::Config> core::ops::Index<Id> for Manager<Config> {
    type Output = Path<Config>;

    #[inline]
    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.as_u8() as usize]
    }
}

impl<Config: endpoint::Config> core::ops::IndexMut<Id> for Manager<Config> {
    #[inline]
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.as_u8() as usize]
    }
}

#[derive(Debug, Eq, PartialEq)]
#[must_use]
pub enum AmplificationOutcome {
    /// The active path was amplification limited and is now not amplification limited
    ActivePathUnblocked,
    /// A path other than the active path was amplification limited and is now not amplification limited
    InactivePathUnblocked,
    /// The path has remained amplification limited or unblocked
    Unchanged,
}

impl AmplificationOutcome {
    /// The active path was amplification limited and is now not amplification limited
    pub fn is_active_path_unblocked(&self) -> bool {
        matches!(self, AmplificationOutcome::ActivePathUnblocked)
    }

    /// A path other than the active path was amplification limited and is now not amplification limited
    pub fn is_inactivate_path_unblocked(&self) -> bool {
        matches!(self, AmplificationOutcome::InactivePathUnblocked)
    }

    /// The path has remained amplification limited or unblocked
    pub fn is_unchanged(&self) -> bool {
        matches!(self, AmplificationOutcome::Unchanged)
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
            is_active: $path.is_active(),
        }
    }};
}

pub(crate) use path_event;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fuzz_target;
