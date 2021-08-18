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
use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::{
    ack, connection, connection::id::AsEvent as _, event, frame, packet::number::PacketNumber,
    stateless_reset, transport,
};
use smallvec::SmallVec;

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#18.2
//# The active connection ID limit is an integer value specifying the
//# maximum number of connection IDs from the peer that an endpoint is
//# willing to store. This value includes the connection ID received
//# during the handshake, that received in the preferred_address transport
//# parameter, and those received in NEW_CONNECTION_ID frames.  The value
//# of the active_connection_id_limit parameter MUST be at least 2.
// A value of 3 is sufficient for a client to probe multiple paths and for a server to respond
// to connection migrations from a client.
pub const ACTIVE_CONNECTION_ID_LIMIT: u8 = 3;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
//# An endpoint SHOULD allow for sending and tracking a number of
//# RETIRE_CONNECTION_ID frames of at least twice the active_connection_id limit.
const RETIRED_CONNECTION_ID_LIMIT: u8 = ACTIVE_CONNECTION_ID_LIMIT * 2;

#[derive(Debug)]
pub struct PeerIdRegistry {
    /// The internal connection ID for this registration
    internal_id: InternalConnectionId,
    /// The shared state between mapper and registration
    state: Rc<RefCell<ConnectionIdMapperState>>,
    /// The connection IDs which are currently registered
    registered_ids: SmallVec<[PeerIdInfo; NR_STATIC_REGISTRABLE_IDS]>,
    /// The largest retire prior to value that has been received from the peer
    retire_prior_to: u32,
}

#[derive(Debug, Clone)]
struct PeerIdInfo {
    id: connection::PeerId,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //# Each Connection ID has an associated sequence number to assist in
    //# detecting when NEW_CONNECTION_ID or RETIRE_CONNECTION_ID frames refer
    //# to the same value.
    sequence_number: u32,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //# A 128-bit value that will be used for a stateless reset when the
    //# associated connection ID is used.
    stateless_reset_token: Option<stateless_reset::Token>,
    // The current status of the connection ID
    status: PeerIdStatus,
}

impl PeerIdInfo {
    /// Returns true if this ID is ready to be retired
    fn is_retire_ready(&self, retire_prior_to: u32) -> bool {
        self.status.is_active() && self.sequence_number < retire_prior_to
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
    //# previously issued connection ID with a different Stateless Reset
    //# Token or a different sequence number, or if a sequence number is used
    //# for different connection IDs, the endpoint MAY treat that receipt as
    //# a connection error of type PROTOCOL_VIOLATION.
    fn validate_new_connection_id(
        &self,
        new_id: &connection::PeerId,
        stateless_reset_token: &stateless_reset::Token,
        sequence_number: u32,
    ) -> Result<bool, PeerIdRegistrationError> {
        let reset_token_is_equal = self
            .stateless_reset_token
            .map_or(false, |token| token == *stateless_reset_token);
        let sequence_number_is_equal = self.sequence_number == sequence_number;

        if self.id == *new_id {
            if !reset_token_is_equal || !sequence_number_is_equal {
                return Err(InvalidNewConnectionId);
            }

            // This was a valid duplicate new connection ID
            return Ok(true);
        } else if sequence_number_is_equal || reset_token_is_equal {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.2
            //# Endpoints are not required to compare new values
            //# against all previous values, but a duplicate value MAY be treated as
            //# a connection error of type PROTOCOL_VIOLATION.
            return Err(InvalidNewConnectionId);
        }

        // This was a valid non-duplicate new connection ID
        Ok(false)
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

impl PeerIdStatus {
    /// Returns true if this PeerId may be used to send packets to the peer
    fn is_active(&self) -> bool {
        matches!(self, New | InUse | InUsePendingNewConnectionId)
    }

    /// Returns true if the status of this ID allows for transmission
    /// based on the transmission constraint
    fn can_transmit(&self, constraint: transmission::Constraint) -> bool {
        match self {
            PendingRetirementRetransmission => constraint.can_retransmit(),
            PendingRetirement => constraint.can_transmit(),
            _ => false,
        }
    }
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
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
            //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
            //# previously issued connection ID with a different Stateless Reset
            //# Token or a different sequence number, or if a sequence number is used
            //# for different connection IDs, the endpoint MAY treat that receipt as
            //# a connection error of type PROTOCOL_VIOLATION.
            PeerIdRegistrationError::InvalidNewConnectionId => transport::Error::PROTOCOL_VIOLATION,
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# After processing a NEW_CONNECTION_ID frame and
            //# adding and retiring active connection IDs, if the number of active
            //# connection IDs exceeds the value advertised in its
            //# active_connection_id_limit transport parameter, an endpoint MUST
            //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
            PeerIdRegistrationError::ExceededActiveConnectionIdLimit => {
                transport::Error::CONNECTION_ID_LIMIT_ERROR
            }
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
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
        let mut guard = self.state.borrow_mut();

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

impl PeerIdRegistry {
    /// Constructs a new `PeerIdRegistry`. The provided `initial_connection_id` will be registered
    /// in the returned registry, with the optional associated `stateless_reset_token`.
    pub(crate) fn new(
        internal_id: InternalConnectionId,
        state: Rc<RefCell<ConnectionIdMapperState>>,
        initial_connection_id: connection::PeerId,
        stateless_reset_token: Option<stateless_reset::Token>,
    ) -> Self {
        let mut registry = Self {
            internal_id,
            state,
            registered_ids: SmallVec::new(),
            retire_prior_to: 0,
        };

        registry.registered_ids.push(PeerIdInfo {
            id: initial_connection_id,
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# The sequence number of the initial connection ID is 0.
            sequence_number: 0,
            stateless_reset_token,
            // Start the initial PeerId in ActivePendingNewConnectionId so the ID used
            // during the handshake is rotated as soon as the peer sends a new connection ID
            status: PeerIdStatus::InUsePendingNewConnectionId,
        });

        if let Some(token) = stateless_reset_token {
            registry
                .state
                .borrow_mut()
                .stateless_reset_map
                .insert(token, internal_id);
        }

        registry
    }

    /// Handles a new connection ID received from a NEW_CONNECTION_ID frame
    pub fn on_new_connection_id(
        &mut self,
        new_id: &connection::PeerId,
        sequence_number: u32,
        retire_prior_to: u32,
        stateless_reset_token: &stateless_reset::Token,
    ) -> Result<(), PeerIdRegistrationError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
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
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
                //# Upon receipt of an increased Retire Prior To field, the peer MUST
                //# stop using the corresponding connection IDs and retire them with
                //# RETIRE_CONNECTION_ID frames before adding the newly provided
                //# connection ID to the set of active connection IDs.
                id_info.status = PendingRetirement;
            }

            if id_info.status.is_active() {
                active_id_count += 1;
            }

            if id_pending_new_connection_id.is_none()
                && id_info.status == InUsePendingNewConnectionId
            {
                id_pending_new_connection_id = Some(id_info);
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //# Receipt of the same frame multiple times MUST NOT be treated as a
        //# connection error.
        if !is_duplicate {
            let mut new_id_info = PeerIdInfo {
                id: *new_id,
                sequence_number,
                stateless_reset_token: Some(*stateless_reset_token),
                status: New,
            };

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
            //# An endpoint that receives a NEW_CONNECTION_ID frame with a sequence
            //# number smaller than the Retire Prior To field of a previously
            //# received NEW_CONNECTION_ID frame MUST send a corresponding
            //# RETIRE_CONNECTION_ID frame that retires the newly received connection
            //# ID, unless it has already done so for that sequence number.
            if new_id_info.is_retire_ready(self.retire_prior_to) {
                new_id_info.status = PendingRetirement;
            }

            if new_id_info.status.is_active() {
                active_id_count += 1;

                if let Some(mut id_pending_new_connection_id) = id_pending_new_connection_id {
                    // If there was an ID pending new connection ID, it can be moved to PendingRetirement
                    // now that we know we aren't processing a duplicate NEW_CONNECTION_ID and the
                    // new connection ID wasn't immediately retired.
                    id_pending_new_connection_id.status = PendingRetirement;
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

        self.ensure_no_duplicates();

        Ok(())
    }

    /// Writes any RETIRE_CONNECTION_ID frames necessary to the given context
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let constraint = context.transmission_constraint();

        for id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| id_info.status.can_transmit(constraint))
        {
            if let Some(packet_number) = context.write_frame(&frame::RetireConnectionId {
                sequence_number: id_info.sequence_number.into(),
            }) {
                id_info.status = PendingAcknowledgement(packet_number);
            }
        }
    }

    /// Removes connection IDs that were pending acknowledgement
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        let mut mapper_state = self.state.borrow_mut();

        self.registered_ids.retain(|id_info| {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    if let Some(token) = id_info.stateless_reset_token {
                        // Stop tracking the stateless reset token
                        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
                        //# An endpoint MUST NOT check for any Stateless Reset Tokens associated
                        //# with connection IDs it has not used or for connection IDs that have
                        //# been retired.
                        mapper_state.stateless_reset_map.remove(&token);
                    }
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
                    //# An endpoint MUST NOT forget a connection ID without retiring it
                    // Don't retain the ID since the retirement was acknowledged
                    return false;
                }
            }

            // Retain IDs that weren't PendingAcknowledgement or weren't acknowledged
            true
        });
    }

    /// Sets the retransmit flag to true for connection IDs pending acknowledgement with a lost
    /// packet number
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        for id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = PendingRetirementRetransmission;
                }
            }
        }
    }

    /// Checks if the peer_id exists and if it is active.
    pub fn is_active(&self, peer_id: &connection::PeerId) -> bool {
        self.registered_ids
            .iter()
            .any(|id_info| peer_id == &id_info.id && id_info.status.is_active())
    }

    /// Tries to consume a new peer_id if one is available.
    ///
    /// Register the stateless reset token once a connection ID is in use.
    pub fn consume_new_id<Pub: event::Publisher>(
        &mut self,
        path_id: path::Id,
        current_peer_connection_id: connection::PeerId,
        publisher: &mut Pub,
    ) -> Option<connection::PeerId> {
        for id_info in self.registered_ids.iter_mut() {
            if id_info.status == New {
                debug_assert_ne!(current_peer_connection_id, id_info.id);

                // Start tracking the stateless reset token
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
                //# An endpoint MUST NOT check for any Stateless Reset Tokens associated
                //# with connection IDs it has not used or for connection IDs that have
                //# been retired.
                if let Some(token) = id_info.stateless_reset_token {
                    self.state
                        .borrow_mut()
                        .stateless_reset_map
                        .insert(token, self.internal_id);
                }

                publisher.on_connection_id_updated(event::builders::ConnectionIdUpdated {
                    path_id: path_id.as_u8() as u64,
                    cid_consumer: event::common::Endpoint::Local,
                    previous: current_peer_connection_id.as_event(),
                    current: id_info.id.as_event(),
                });

                // Consume the new id
                id_info.status = InUse;
                return Some(id_info.id);
            }
        }

        None
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
                .filter(|id_info| id_info.status.is_active())
                .count()
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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
                .filter(|id_info| !id_info.status.is_active())
                .count()
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //# An endpoint SHOULD limit the number of connection IDs it has retired
        //# locally and have not yet been acknowledged. An endpoint SHOULD allow
        //# for sending and tracking a number of RETIRE_CONNECTION_ID frames of
        //# at least twice the active_connection_id limit.  An endpoint MUST NOT
        //# forget a connection ID without retiring it, though it MAY choose to
        //# treat having connection IDs in need of retirement that exceed this
        //# limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
        if retired_id_count > RETIRED_CONNECTION_ID_LIMIT as usize {
            return Err(ExceededRetiredConnectionIdLimit);
        }

        Ok(())
    }

    fn ensure_no_duplicates(&self) {
        if cfg!(debug_assertions) {
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
        for id_info in self.registered_ids.iter() {
            match id_info.status {
                PendingRetirement => {
                    query.on_new_data()?;
                }
                PendingRetirementRetransmission => {
                    query.on_lost_data()?;
                    // `LostData` is the highest precedent interest we provide,
                    // so we don't need to keep iterating to check other IDs
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{
        connection::{
            peer_id_registry::{
                PeerIdRegistrationError,
                PeerIdRegistrationError::{
                    ExceededActiveConnectionIdLimit, ExceededRetiredConnectionIdLimit,
                    InvalidNewConnectionId,
                },
                PeerIdStatus::{
                    InUse, InUsePendingNewConnectionId, New, PendingAcknowledgement,
                    PendingRetirement, PendingRetirementRetransmission,
                },
                RETIRED_CONNECTION_ID_LIMIT,
            },
            ConnectionIdMapper, InternalConnectionIdGenerator, PeerIdRegistry,
        },
        contexts::{
            testing::{MockWriteContext, OutgoingFrameBuffer},
            WriteContext,
        },
        path, transmission,
        transmission::interest::Provider,
    };
    use s2n_quic_core::{
        connection, endpoint,
        event::testing::Publisher,
        frame::{new_connection_id::STATELESS_RESET_TOKEN_LEN, Frame, RetireConnectionId},
        packet::number::PacketNumberRange,
        random, stateless_reset,
        stateless_reset::token::testing::*,
        transport,
        varint::VarInt,
    };

    // Helper function to easily generate a PeerId from bytes
    pub fn id(bytes: &[u8]) -> connection::PeerId {
        connection::PeerId::try_from_bytes(bytes).unwrap()
    }

    // Helper function to easily create a PeerIdRegistry
    fn reg(
        initial_id: connection::PeerId,
        stateless_reset_token: Option<stateless_reset::Token>,
    ) -> PeerIdRegistry {
        let mut random_generator = random::testing::Generator(123);

        ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
            .create_peer_id_registry(
                InternalConnectionIdGenerator::new().generate_id(),
                initial_id,
                stateless_reset_token,
            )
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
    //= type=test
    //# An endpoint SHOULD limit the number of connection IDs it has retired
    //# locally and have not yet been acknowledged. An endpoint SHOULD allow
    //# for sending and tracking a number of RETIRE_CONNECTION_ID frames of
    //# at least twice the active_connection_id limit.  An endpoint MUST NOT
    //# forget a connection ID without retiring it, though it MAY choose to
    //# treat having connection IDs in need of retirement that exceed this
    //# limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
    #[test]
    fn error_when_exceeding_retired_connection_id_limit() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        // Register 6 more new IDs for a total of 7, with 6 retired
        for i in 2u32..=(RETIRED_CONNECTION_ID_LIMIT + 1).into() {
            assert!(reg
                .on_new_connection_id(
                    &id(&i.to_ne_bytes()),
                    i,
                    i,
                    &[i as u8; STATELESS_RESET_TOKEN_LEN].into()
                )
                .is_ok());
        }

        // Retiring one more ID exceeds the limit
        let result = reg.on_new_connection_id(
            &id(b"id08"),
            8,
            8,
            &[8_u8; STATELESS_RESET_TOKEN_LEN].into(),
        );

        assert_eq!(Some(ExceededRetiredConnectionIdLimit), result.err());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# After processing a NEW_CONNECTION_ID frame and
    //# adding and retiring active connection IDs, if the number of active
    //# connection IDs exceeds the value advertised in its
    //# active_connection_id_limit transport parameter, an endpoint MUST
    //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
    #[test]
    fn error_when_exceeding_active_connection_id_limit() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        // Register 5 more new IDs with a retire_prior_to of 4, so there will be a total of
        // 6 connection IDs, with 3 retired and 3 active
        for i in 2u32..=6 {
            assert!(reg
                .on_new_connection_id(
                    &id(&i.to_ne_bytes()),
                    i,
                    4,
                    &[i as u8; STATELESS_RESET_TOKEN_LEN].into()
                )
                .is_ok());
        }

        // Adding one more ID exceeds the limit
        let result = reg.on_new_connection_id(
            &id(b"id07"),
            8,
            0,
            &[8_u8; STATELESS_RESET_TOKEN_LEN].into(),
        );

        assert_eq!(Some(ExceededActiveConnectionIdLimit), result.err());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //= type=test
    //# Receipt of the same frame multiple times MUST NOT be treated as a
    //# connection error.
    #[test]
    fn no_error_when_duplicate() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

        assert_eq!(2, reg.registered_ids.len());
        reg.registered_ids[1].status = PendingRetirement;

        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());
        assert_eq!(2, reg.registered_ids.len());
        assert_eq!(PendingRetirement, reg.registered_ids[1].status);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#18.2
    //= type=test
    //# The value of the active_connection_id_limit parameter MUST be at least 2.
    #[test]
    fn active_connection_id_limit_must_be_at_least_2() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

        let id_3 = id(b"id03");
        assert!(reg.on_new_connection_id(&id_3, 2, 0, &TEST_TOKEN_2).is_ok());

        assert_eq!(
            2,
            reg.registered_ids
                .iter()
                .filter(|id_info| id_info.status.is_active())
                .count()
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //= type=test
    //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
    //# previously issued connection ID with a different Stateless Reset
    //# Token or a different sequence number, or if a sequence number is used
    //# for different connection IDs, the endpoint MAY treat that receipt as
    //# a connection error of type PROTOCOL_VIOLATION.
    #[test]
    fn duplicate_new_id_different_token_or_sequence_number() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

        // Change the sequence number
        let mut result = reg.on_new_connection_id(&id_2, 2, 0, &TEST_TOKEN_1);
        assert_eq!(Some(InvalidNewConnectionId), result.err());

        // Change the stateless reset token
        result = reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2);
        assert_eq!(Some(InvalidNewConnectionId), result.err());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //= type=test
    //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
    //# previously issued connection ID with a different Stateless Reset
    //# Token or a different sequence number, or if a sequence number is used
    //# for different connection IDs, the endpoint MAY treat that receipt as
    //# a connection error of type PROTOCOL_VIOLATION.
    #[test]
    fn non_duplicate_new_id_same_token_or_sequence_number() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        let id_3 = id(b"id03");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

        // Same sequence number
        let mut result = reg.on_new_connection_id(&id_3, 1, 0, &TEST_TOKEN_2);
        assert_eq!(Some(InvalidNewConnectionId), result.err());

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.2
        //= type=test
        //# Endpoints are not required to compare new values
        //# against all previous values, but a duplicate value MAY be treated as
        //# a connection error of type PROTOCOL_VIOLATION.
        // Same stateless reset token
        result = reg.on_new_connection_id(&id_3, 2, 0, &TEST_TOKEN_1);
        assert_eq!(Some(InvalidNewConnectionId), result.err());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //= type=test
    //# A receiver MUST ignore any Retire Prior To fields that do not
    //# increase the largest received Retire Prior To value.
    #[test]
    fn ignore_retire_prior_to_that_does_not_increase() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        let id_3 = id(b"id03");
        let id_4 = id(b"id04");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());
        assert!(reg.on_new_connection_id(&id_3, 2, 1, &TEST_TOKEN_3).is_ok());
        assert_eq!(1, reg.retire_prior_to);
        assert!(reg.on_new_connection_id(&id_4, 3, 0, &TEST_TOKEN_4).is_ok());
        assert_eq!(1, reg.retire_prior_to);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
    //= type=test
    //# Upon receipt of an increased Retire Prior To field, the peer MUST
    //# stop using the corresponding connection IDs and retire them with
    //# RETIRE_CONNECTION_ID frames before adding the newly provided
    //# connection ID to the set of active connection IDs.
    #[test]
    fn retire_connection_id_when_retire_prior_to_increases() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let mut reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 1, &TEST_TOKEN_2).is_ok());

        assert_eq!(PendingRetirement, reg.registered_ids[0].status);
        assert_eq!(
            transmission::Interest::NewData,
            reg.get_transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );
        let packet_number = write_context.packet_number();
        reg.on_transmit(&mut write_context);

        let expected_frame = Frame::RetireConnectionId {
            0: RetireConnectionId {
                sequence_number: VarInt::from_u32(0),
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );
        assert_eq!(
            PendingAcknowledgement(packet_number),
            reg.registered_ids[0].status
        );

        assert_eq!(
            transmission::Interest::None,
            reg.get_transmission_interest()
        );

        reg.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            PendingRetirementRetransmission,
            reg.registered_ids[0].status
        );
        assert_eq!(
            transmission::Interest::LostData,
            reg.get_transmission_interest()
        );

        // Transition ID to PendingAcknowledgement again
        let packet_number = write_context.packet_number();
        reg.on_transmit(&mut write_context);

        assert_eq!(
            Some(reg.internal_id),
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
        );

        reg.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        // ID 1 was removed
        assert_eq!(reg.registered_ids.len(), 1);
        assert_eq!(id_2, reg.registered_ids[0].id);
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
        //= type=test
        //# An endpoint MUST NOT check for any Stateless Reset Tokens associated
        //# with connection IDs it has not used or for connection IDs that have
        //# been retired.
        assert_eq!(
            None,
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
        );

        assert_eq!(
            transmission::Interest::None,
            reg.get_transmission_interest()
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
    //= type=test
    //# An endpoint that receives a NEW_CONNECTION_ID frame with a sequence
    //# number smaller than the Retire Prior To field of a previously
    //# received NEW_CONNECTION_ID frame MUST send a corresponding
    //# RETIRE_CONNECTION_ID frame that retires the newly received connection
    //# ID, unless it has already done so for that sequence number.
    #[test]
    fn retire_new_connection_id_if_sequence_number_smaller_than_retire_prior_to() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        let id_2 = id(b"id02");
        assert!(reg
            .on_new_connection_id(&id_2, 10, 10, &TEST_TOKEN_2)
            .is_ok());
        assert_eq!(PendingRetirement, reg.registered_ids[0].status);
        assert_eq!(New, reg.registered_ids[1].status);

        let id_3 = id(b"id03");
        assert!(reg.on_new_connection_id(&id_3, 1, 0, &TEST_TOKEN_3).is_ok());

        assert_eq!(New, reg.registered_ids[1].status);
        assert_eq!(PendingRetirement, reg.registered_ids[2].status);
    }

    #[test]
    fn retire_initial_id_when_new_connection_id_available() {
        let id_1 = id(b"id01");
        let mut reg = reg(id_1, None);

        assert_eq!(InUsePendingNewConnectionId, reg.registered_ids[0].status);

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());

        assert_eq!(PendingRetirement, reg.registered_ids[0].status);
    }

    #[test]
    pub fn initial_id_is_active() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        assert!(reg.is_active(&id_1));
    }

    #[test]
    pub fn retired_id_is_not_active() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let mut reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        assert!(reg.is_active(&id_1));
        reg.registered_ids[0].status = PendingRetirement;
        assert!(!reg.is_active(&id_1));
    }

    #[test]
    pub fn unknown_id_is_not_active() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        assert!(reg.is_active(&id_1));
        let id_unknown = id(b"unknown");
        assert!(!reg.is_active(&id_unknown));
    }

    #[test]
    pub fn consume_new_id_should_return_id() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let mut reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        let id_2 = id(b"id02");
        assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());
        reg.registered_ids[1].status = New;

        assert!(reg
            .state
            .borrow_mut()
            .stateless_reset_map
            .remove(&TEST_TOKEN_2)
            .is_none());
        assert_eq!(
            Some(id_2),
            reg.consume_new_id(path::Id::test_id(), id_1, &mut Publisher)
        );
        reg.registered_ids[1].status = InUse;
        // this is an indirect way to test that we inserted a reset token when we consumed id_2
        assert!(reg
            .state
            .borrow_mut()
            .stateless_reset_map
            .remove(&TEST_TOKEN_2)
            .is_some());
    }

    #[test]
    pub fn consume_new_id_should_error_if_no_ids_are_available() {
        let id_1 = id(b"id01");
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let mut reg = mapper.create_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            id_1,
            Some(TEST_TOKEN_1),
        );

        assert_eq!(
            None,
            reg.consume_new_id(path::Id::test_id(), id_1, &mut Publisher)
        );
    }

    #[test]
    fn error_conversion() {
        let mut transport_error: transport::Error;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
        //= type=test
        //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
        //# previously issued connection ID with a different Stateless Reset
        //# Token or a different sequence number, or if a sequence number is used
        //# for different connection IDs, the endpoint MAY treat that receipt as
        //# a connection error of type PROTOCOL_VIOLATION.
        transport_error = PeerIdRegistrationError::InvalidNewConnectionId.into();
        assert_eq!(
            transport::Error::PROTOCOL_VIOLATION.code,
            transport_error.code
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //= type=test
        //# After processing a NEW_CONNECTION_ID frame and
        //# adding and retiring active connection IDs, if the number of active
        //# connection IDs exceeds the value advertised in its
        //# active_connection_id_limit transport parameter, an endpoint MUST
        //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
        transport_error = PeerIdRegistrationError::ExceededActiveConnectionIdLimit.into();
        assert_eq!(
            transport::Error::CONNECTION_ID_LIMIT_ERROR.code,
            transport_error.code
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //= type=test
        //# An endpoint MUST NOT forget a connection ID without retiring it,
        //# though it MAY choose to treat having connection IDs in need of
        //# retirement that exceed this limit as a connection error of type
        //# CONNECTION_ID_LIMIT_ERROR.
        transport_error = PeerIdRegistrationError::ExceededRetiredConnectionIdLimit.into();
        assert_eq!(
            transport::Error::CONNECTION_ID_LIMIT_ERROR.code,
            transport_error.code
        );
    }
}
