// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{
        connection_id_mapper::ConnectionIdMapperState,
        peer_id_registry::{
            PeerIdRegistrationError::{
                ExceededActiveConnectionIdLimit, ExceededRetiredConnectionIdLimit,
                InvalidNewConnectionId,
            },
            PeerIdStatus::{
                InUse, InUsePendingNewConnectionId, New, PendingAcknowledgement, PendingRetirement,
                PendingRetirementRetransmission,
            },
        },
        InternalConnectionId,
    },
    path,
    transmission::{self, WriteContext},
};
use s2n_quic_core::{
    ack, connection, endpoint,
    event::{self, IntoEvent},
    frame,
    memo::Memo,
    packet::number::PacketNumber,
    stateless_reset, transport,
};
use smallvec::SmallVec;
use std::sync::{Arc, Mutex};

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

//= https://www.rfc-editor.org/rfc/rfc9000#section-18.2
//# This is an integer value
//# specifying the maximum number of connection IDs from the peer that
//# an endpoint is willing to store.  This value includes the
//# connection ID received during the handshake, that received in the
//# preferred_address transport parameter, and those received in
//# NEW_CONNECTION_ID frames.  The value of the
//# active_connection_id_limit parameter MUST be at least 2.
// A value of 3 is sufficient for a client to probe multiple paths and for a server to respond
// to connection migrations from a client.
pub const ACTIVE_CONNECTION_ID_LIMIT: u8 = 3;

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
//# An endpoint SHOULD allow for sending and tracking a
//# number of RETIRE_CONNECTION_ID frames of at least twice the value of
//# the active_connection_id_limit transport parameter.
const RETIRED_CONNECTION_ID_LIMIT: u8 = ACTIVE_CONNECTION_ID_LIMIT * 2;

#[derive(Debug)]
pub struct PeerIdRegistry {
    /// The internal connection ID for this registration
    internal_id: InternalConnectionId,
    /// The shared state between mapper and registration
    state: Arc<Mutex<ConnectionIdMapperState>>,
    /// The connection IDs which are currently registered
    registered_ids: RegisteredIds,
    /// The largest retire prior to value that has been received from the peer
    retire_prior_to: u32,
    /// Memoized query to track if there is any ACK interest
    ack_interest: Memo<bool, RegisteredIds>,
    /// Memoized query to track if there is any transmission interest
    transmission_interest: Memo<transmission::Interest, RegisteredIds>,
    /// If true, the connection ID used during the the handshake will be retired
    /// when the peer sends a NEW_CONNECTION_ID frame.
    rotate_handshake_connection_id: bool,
}

type RegisteredIds = SmallVec<[PeerIdInfo; NR_STATIC_REGISTRABLE_IDS]>;

#[derive(Debug, Clone)]
struct PeerIdInfo {
    id: connection::PeerId,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //# Each Connection ID has an associated sequence number to assist in
    //# detecting when NEW_CONNECTION_ID or RETIRE_CONNECTION_ID frames refer
    //# to the same value.
    sequence_number: u32,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
    //# A 128-bit value that will be used for a stateless reset when the
    //# associated connection ID is used.
    stateless_reset_token: Option<stateless_reset::Token>,
    // The current status of the connection ID
    status: PeerIdStatus,
}

impl PeerIdInfo {
    /// Returns true if this ID is ready to be retired
    fn is_retire_ready(&self, retire_prior_to: u32) -> bool {
        self.is_active() && self.sequence_number < retire_prior_to
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
    //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
    //# previously issued connection ID with a different Stateless Reset
    //# Token field value or a different Sequence Number field value, or if a
    //# sequence number is used for different connection IDs, the endpoint
    //# MAY treat that receipt as a connection error of type
    //# PROTOCOL_VIOLATION.
    fn validate_new_connection_id(
        &self,
        new_id: &connection::PeerId,
        stateless_reset_token: &stateless_reset::Token,
        sequence_number: u32,
    ) -> Result<bool, PeerIdRegistrationError> {
        let reset_token_is_equal = self.stateless_reset_token == Some(*stateless_reset_token);
        let sequence_number_is_equal = self.sequence_number == sequence_number;

        if self.id == *new_id {
            if !reset_token_is_equal || !sequence_number_is_equal {
                return Err(InvalidNewConnectionId);
            }

            // This was a valid duplicate new connection ID
            return Ok(true);
        } else if sequence_number_is_equal || reset_token_is_equal {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.2
            //# Endpoints are not required to compare new values
            //# against all previous values, but a duplicate value MAY be treated as
            //# a connection error of type PROTOCOL_VIOLATION.
            return Err(InvalidNewConnectionId);
        }

        // This was a valid non-duplicate new connection ID
        Ok(false)
    }

    /// Returns true if this PeerId may be used to send packets to the peer
    fn is_active(&self) -> bool {
        matches!(self.status, New | InUse | InUsePendingNewConnectionId)
    }

    /// Returns true if the status of this ID allows for transmission
    /// based on the transmission constraint
    fn transmission_interest(&self) -> transmission::Interest {
        match self.status {
            PendingRetirementRetransmission => transmission::Interest::LostData,
            PendingRetirement => transmission::Interest::NewData,
            _ => transmission::Interest::None,
        }
    }
}

/// The current status of the connection ID.
#[derive(Clone, Debug, PartialEq)]
enum PeerIdStatus {
    /// Connection IDs received in NEW_CONNECTION_ID frames start in the `New` status.
    New,
    /// Once a connection ID is used on a path it moves to the `InUse` status.
    InUse,
    /// The initial connection ID used during the handshake is in use, but will be retired
    /// as soon as a NEW_CONNECTION_ID frame is received from the peer.
    InUsePendingNewConnectionId,
    /// Once a connection ID will no longer be used, it enters the `PendingRetirement` status,
    /// triggering a RETIRE_CONNECTION_ID frame to be sent.
    PendingRetirement,
    /// If the packet that sent the RETIRE_CONNECTION_ID frame was declared lost, the connection
    /// moves to the `PendingRetirementRetransmission` status to allow for faster retransmission
    /// of the lost frame.
    PendingRetirementRetransmission,
    /// Once the RETIRE_CONNECTION_ID frame has been sent, the connection ID enters
    /// `PendingAcknowledgement` status, tracking the packet number of the packet that transmitted
    /// the retire frame. When acknowledgement of that packet is received, the connection ID is
    /// removed.
    PendingAcknowledgement(PacketNumber),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerIdRegistrationError {
    /// The NEW_CONNECTION_ID frame was invalid
    InvalidNewConnectionId,
    /// The active_connection_id_limit was exceeded
    ExceededActiveConnectionIdLimit,
    /// Too many connection IDs are pending retirement
    ExceededRetiredConnectionIdLimit,
}

impl PeerIdRegistrationError {
    fn message(&self) -> &'static str {
        match self {
            PeerIdRegistrationError::InvalidNewConnectionId => {
                "The new connection ID had an invalid sequence_number or stateless_reset_token"
            }
            PeerIdRegistrationError::ExceededActiveConnectionIdLimit => {
                "The active_connection_id_limit has been exceeded"
            }
            PeerIdRegistrationError::ExceededRetiredConnectionIdLimit => {
                "Too many connection IDs have been retired without acknowledgement from the peer"
            }
        }
    }
}

impl From<PeerIdRegistrationError> for transport::Error {
    fn from(err: PeerIdRegistrationError) -> Self {
        let transport_error = match err {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
            //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
            //# previously issued connection ID with a different Stateless Reset
            //# Token field value or a different Sequence Number field value, or if a
            //# sequence number is used for different connection IDs, the endpoint
            //# MAY treat that receipt as a connection error of type
            //# PROTOCOL_VIOLATION.
            PeerIdRegistrationError::InvalidNewConnectionId => transport::Error::PROTOCOL_VIOLATION,
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
            //# After processing a NEW_CONNECTION_ID frame and
            //# adding and retiring active connection IDs, if the number of active
            //# connection IDs exceeds the value advertised in its
            //# active_connection_id_limit transport parameter, an endpoint MUST
            //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
            PeerIdRegistrationError::ExceededActiveConnectionIdLimit => {
                transport::Error::CONNECTION_ID_LIMIT_ERROR
            }
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
            //# An endpoint MUST NOT forget a connection ID without retiring it,
            //# though it MAY choose to treat having connection IDs in need of
            //# retirement that exceed this limit as a connection error of type
            //# CONNECTION_ID_LIMIT_ERROR.
            PeerIdRegistrationError::ExceededRetiredConnectionIdLimit => {
                transport::Error::CONNECTION_ID_LIMIT_ERROR
            }
        };
        transport_error.with_reason(err.message())
    }
}

impl Drop for PeerIdRegistry {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            // Stop tracking all associated stateless reset tokens
            for token in self
                .registered_ids
                .iter()
                .flat_map(|id_info| id_info.stateless_reset_token)
            {
                guard.stateless_reset_map.remove(&token);
            }
        }
    }
}

impl PeerIdRegistry {
    /// Constructs a new `PeerIdRegistry`.
    pub(crate) fn new(
        internal_id: InternalConnectionId,
        state: Arc<Mutex<ConnectionIdMapperState>>,
        rotate_handshake_connection_id: bool,
    ) -> Self {
        Self {
            internal_id,
            state,
            registered_ids: SmallVec::new(),
            retire_prior_to: 0,
            ack_interest: Memo::new(|ids| {
                for id in ids.iter() {
                    if matches!(id.status, PendingAcknowledgement(_)) {
                        return true;
                    }
                }

                false
            }),
            transmission_interest: Memo::new(|ids| {
                let mut interest = transmission::Interest::None;

                for id_info in ids {
                    interest = interest.max(id_info.transmission_interest());
                }

                interest
            }),
            rotate_handshake_connection_id,
        }
    }

    /// Used to register the initial peer DestinationConnectionId.
    ///
    /// For a Server endpoint this happens immediately after creation of the
    /// PeerIdRegistry, since the ClientHello includes a SourceConnectionId.
    /// A Client endpoint must however wait for the initial Server response
    /// to populate this value.
    pub(crate) fn register_initial_connection_id(&mut self, peer_id: connection::PeerId) {
        debug_assert!(self.is_empty());

        let status = if self.rotate_handshake_connection_id {
            // Start the initial PeerId in `InUsePendingNewConnectionId` so the ID used
            // during the handshake is rotated as soon as the peer sends a new connection ID
            PeerIdStatus::InUsePendingNewConnectionId
        } else {
            PeerIdStatus::InUse
        };

        self.registered_ids.push(PeerIdInfo {
            id: peer_id,
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
            //# The sequence number of the initial connection ID is 0.
            sequence_number: 0,
            stateless_reset_token: None,

            status,
        });

        self.check_consistency();
    }

    /// Used to register the initial peer stateless reset token that applies to the connection ID
    /// the server selected during the handshake.
    ///
    /// This method is only used on the client to register a stateless token from a peer server,
    /// as clients cannot transmit a stateless reset token in their transport parameters due to
    /// lack of confidentiality protection.
    pub(crate) fn register_initial_stateless_reset_token(
        &mut self,
        stateless_reset_token: stateless_reset::Token,
    ) {
        debug_assert!(!self.is_empty());

        if let Some(peer_id_info) = self.registered_ids.get_mut(0) {
            debug_assert_eq!(None, peer_id_info.stateless_reset_token);
            peer_id_info.stateless_reset_token = Some(stateless_reset_token);
        }

        self.state
            .lock()
            .expect("should succeed unless the lock is poisoned")
            .stateless_reset_map
            .insert(stateless_reset_token, self.internal_id);
    }

    /// Check if registered_ids is empty.
    ///
    /// This is only expected to be true when an endpoint creates a new
    /// peer_id_registry.
    pub(crate) fn is_empty(&self) -> bool {
        self.registered_ids.is_empty()
    }

    /// Handles a new connection ID received from a NEW_CONNECTION_ID frame
    pub fn on_new_connection_id(
        &mut self,
        new_id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: &stateless_reset::Token,
    ) -> Result<(), PeerIdRegistrationError> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
        //# A receiver MUST ignore any Retire Prior To fields that do not
        //# increase the largest received Retire Prior To value.
        self.retire_prior_to = self.retire_prior_to.max(retire_prior_to);

        let mut active_id_count = 0;
        let mut is_duplicate = false;
        let mut id_pending_new_connection_id = None;

        // Iterate over all registered IDs, retiring any as necessary
        for id_info in self.registered_ids.iter_mut() {
            is_duplicate |= id_info.validate_new_connection_id(
                new_id,
                stateless_reset_token,
                sequence_number,
            )?;

            if id_info.is_retire_ready(self.retire_prior_to) {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
                //# Upon receipt of an increased Retire Prior To field, the peer MUST
                //# stop using the corresponding connection IDs and retire them with
                //# RETIRE_CONNECTION_ID frames before adding the newly provided
                //# connection ID to the set of active connection IDs.
                id_info.status = PendingRetirement;
                self.transmission_interest.clear();
            }

            if id_info.is_active() {
                active_id_count += 1;
            }

            if id_pending_new_connection_id.is_none()
                && id_info.status == InUsePendingNewConnectionId
            {
                id_pending_new_connection_id = Some(id_info);
            }
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
        //# Receipt of the same frame multiple times MUST NOT be treated as a
        //# connection error.
        if !is_duplicate {
            let mut new_id_info = PeerIdInfo {
                id: *new_id,
                sequence_number,
                stateless_reset_token: Some(*stateless_reset_token),
                status: New,
            };

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
            //# An endpoint that receives a NEW_CONNECTION_ID frame with a sequence
            //# number smaller than the Retire Prior To field of a previously
            //# received NEW_CONNECTION_ID frame MUST send a corresponding
            //# RETIRE_CONNECTION_ID frame that retires the newly received connection
            //# ID, unless it has already done so for that sequence number.
            if new_id_info.is_retire_ready(self.retire_prior_to) {
                new_id_info.status = PendingRetirement;
                self.transmission_interest.clear();
            }

            if new_id_info.is_active() {
                active_id_count += 1;

                if let Some(id_pending_new_connection_id) = id_pending_new_connection_id {
                    // If there was an ID pending new connection ID, it can be moved to PendingRetirement
                    // now that we know we aren't processing a duplicate NEW_CONNECTION_ID and the
                    // new connection ID wasn't immediately retired.
                    id_pending_new_connection_id.status = PendingRetirement;
                    self.transmission_interest.clear();
                    // We retired one active connection ID
                    active_id_count -= 1;
                }
            }

            self.registered_ids.push(new_id_info);

            self.check_active_connection_id_limit(active_id_count)?;
        }

        // Duplicate NEW_CONNECTION_ID frames may not change the sequence number or
        // stateless reset token, but the RFC does not specify the behavior if the
        // retire prior to value changes. This means the number of retired connection
        // IDs may have changed even if the NEW_CONNECTION_ID frame was a duplicate,
        // so we will validate the retired id count regardless of the duplicate status.
        let retired_id_count = self.registered_ids.len() - active_id_count;

        self.check_retired_connection_id_limit(retired_id_count)?;

        self.check_consistency();

        Ok(())
    }

    /// Writes any RETIRE_CONNECTION_ID frames necessary to the given context
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let constraint = context.transmission_constraint();

        if !self
            .transmission_interest
            .get(&self.registered_ids)
            .can_transmit(constraint)
        {
            return;
        }

        for id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| id_info.transmission_interest().can_transmit(constraint))
        {
            if let Some(packet_number) = context.write_frame(&frame::RetireConnectionId {
                sequence_number: id_info.sequence_number.into(),
            }) {
                id_info.status = PendingAcknowledgement(packet_number);
                self.transmission_interest.clear();
                self.ack_interest.clear();
            }
        }

        self.check_consistency();
    }

    /// Removes connection IDs that were pending acknowledgement
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        if !self.ack_interest.get(&self.registered_ids) {
            return;
        }

        let mut mapper_state = self
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned");

        self.registered_ids.retain(|id_info| {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    if let Some(token) = id_info.stateless_reset_token {
                        // Stop tracking the stateless reset token
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
                        //# An endpoint MUST NOT check for any stateless reset tokens associated
                        //# with connection IDs it has not used or for connection IDs that have
                        //# been retired.
                        mapper_state.stateless_reset_map.remove(&token);
                    }

                    self.ack_interest.clear();

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
                    //# An endpoint MUST NOT forget a connection ID without retiring it
                    // Don't retain the ID since the retirement was acknowledged
                    return false;
                }
            }

            // Retain IDs that weren't PendingAcknowledgement or weren't acknowledged
            true
        });

        self.check_consistency();
    }

    /// Sets the retransmit flag to true for connection IDs pending acknowledgement with a lost
    /// packet number
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if !self.ack_interest.get(&self.registered_ids) {
            return;
        }

        for id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = PendingRetirementRetransmission;
                    self.ack_interest.clear();
                    self.transmission_interest.clear();
                }
            }
        }

        self.check_consistency();
    }

    /// Checks if the peer_id exists and if it is active.
    pub fn is_active(&self, peer_id: &connection::PeerId) -> bool {
        self.registered_ids
            .iter()
            .any(|id_info| peer_id == &id_info.id && id_info.is_active())
    }

    /// Tries to consume a new peer_id if one is available.
    ///
    /// Register the stateless reset token once a connection ID is in use.
    fn consume_new_id_inner(&mut self) -> Option<connection::PeerId> {
        for id_info in self.registered_ids.iter_mut() {
            if id_info.status == New {
                // Start tracking the stateless reset token
                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
                //# An endpoint MUST NOT check for any stateless reset tokens associated
                //# with connection IDs it has not used or for connection IDs that have
                //# been retired.
                if let Some(token) = id_info.stateless_reset_token {
                    self.state
                        .lock()
                        .expect("should succeed unless the lock is poisoned")
                        .stateless_reset_map
                        .insert(token, self.internal_id);
                }

                // Consume the new id
                id_info.status = InUse;
                return Some(id_info.id);
            }
        }

        None
    }

    /// Tries to consume a new peer_id if one is available for an existing path.
    pub fn consume_new_id_for_existing_path<Pub: event::ConnectionPublisher>(
        &mut self,
        path_id: path::Id,
        current_peer_connection_id: connection::PeerId,
        publisher: &mut Pub,
    ) -> Option<connection::PeerId> {
        let new_id = self.consume_new_id_inner();
        if let Some(new_id) = new_id {
            debug_assert_ne!(current_peer_connection_id, new_id);

            publisher.on_connection_id_updated(event::builder::ConnectionIdUpdated {
                path_id: path_id.into_event(),
                cid_consumer: endpoint::Location::Local,
                previous: current_peer_connection_id.into_event(),
                current: new_id.into_event(),
            });
        }
        new_id
    }

    /// Tries to consume a new peer_id if one is available for a new path.
    pub fn consume_new_id_for_new_path(&mut self) -> Option<connection::PeerId> {
        self.consume_new_id_inner()
    }

    // Validate that the ACTIVE_CONNECTION_ID_LIMIT has not been exceeded
    fn check_active_connection_id_limit(
        &self,
        active_id_count: usize,
    ) -> Result<(), PeerIdRegistrationError> {
        debug_assert_eq!(
            active_id_count,
            self.registered_ids
                .iter()
                .filter(|id_info| id_info.is_active())
                .count()
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# After processing a NEW_CONNECTION_ID frame and
        //# adding and retiring active connection IDs, if the number of active
        //# connection IDs exceeds the value advertised in its
        //# active_connection_id_limit transport parameter, an endpoint MUST
        //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
        if active_id_count > ACTIVE_CONNECTION_ID_LIMIT as usize {
            return Err(ExceededActiveConnectionIdLimit);
        }

        Ok(())
    }

    // Validate that the RETIRED_CONNECTION_ID_LIMIT has not been exceeded
    fn check_retired_connection_id_limit(
        &self,
        retired_id_count: usize,
    ) -> Result<(), PeerIdRegistrationError> {
        debug_assert_eq!(
            retired_id_count,
            self.registered_ids
                .iter()
                .filter(|id_info| !id_info.is_active())
                .count()
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
        //# An endpoint SHOULD limit the number of connection IDs it has retired
        //# locally for which RETIRE_CONNECTION_ID frames have not yet been
        //# acknowledged.  An endpoint SHOULD allow for sending and tracking a
        //# number of RETIRE_CONNECTION_ID frames of at least twice the value of
        //# the active_connection_id_limit transport parameter.  An endpoint MUST
        //# NOT forget a connection ID without retiring it, though it MAY choose
        //# to treat having connection IDs in need of retirement that exceed this
        //# limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
        if retired_id_count > RETIRED_CONNECTION_ID_LIMIT as usize {
            return Err(ExceededRetiredConnectionIdLimit);
        }

        Ok(())
    }

    fn check_consistency(&self) {
        if cfg!(debug_assertions) {
            self.ack_interest.check_consistency(&self.registered_ids);
            self.transmission_interest
                .check_consistency(&self.registered_ids);

            let before_count = self.registered_ids.len();
            let mut registered_id_copy = self.registered_ids.to_vec();
            registered_id_copy.sort_by_key(|id_info| id_info.sequence_number);
            registered_id_copy.dedup_by_key(|id_info| id_info.sequence_number);
            assert_eq!(before_count, registered_id_copy.len());
            registered_id_copy.sort_by_key(|id_info| id_info.id);
            registered_id_copy.dedup_by_key(|id_info| id_info.id);
            assert_eq!(before_count, registered_id_copy.len());
            registered_id_copy.sort_by_key(|id_info| {
                id_info
                    .stateless_reset_token
                    .map(|token| token.into_inner())
            });
            registered_id_copy.dedup_by_key(|id_info| id_info.stateless_reset_token);
            assert_eq!(before_count, registered_id_copy.len());
        }
    }
}

impl transmission::interest::Provider for PeerIdRegistry {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        let interest = self.transmission_interest.get(&self.registered_ids);
        query.on_interest(interest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::connection::{ConnectionIdMapper, InternalConnectionIdGenerator, PeerIdRegistry};
    use s2n_quic_core::{connection, endpoint, random, stateless_reset};

    // Helper function to easily generate a PeerId from bytes
    pub fn id(bytes: &[u8]) -> connection::PeerId {
        connection::PeerId::try_from_bytes(bytes).unwrap()
    }

    // Helper function to easily create a PeerIdRegistry
    pub(crate) fn peer_registry(
        initial_id: connection::PeerId,
        stateless_reset_token: Option<stateless_reset::Token>,
    ) -> PeerIdRegistry {
        let mut random_generator = random::testing::Generator(123);

        let mut registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
            .create_client_peer_id_registry(
                InternalConnectionIdGenerator::new().generate_id(),
                true,
            );
        registry.register_initial_connection_id(initial_id);
        if let Some(token) = stateless_reset_token {
            registry.register_initial_stateless_reset_token(token);
        }
        registry
    }
}
