// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{
        connection_id_mapper::ConnectionIdMapperState, local_id_registry::LocalIdStatus::*,
        InternalConnectionId,
    },
    contexts::WriteContext,
    transmission,
};
use core::convert::TryInto;
use s2n_quic_core::{
    ack, connection, frame,
    memo::Memo,
    packet::number::PacketNumber,
    stateless_reset,
    time::{timer, Duration, Timer, Timestamp},
};
use smallvec::SmallVec;
use std::sync::{Arc, Mutex};

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//# An endpoint that initiates migration and requires non-zero-length
//# connection IDs SHOULD ensure that the pool of connection IDs
//# available to its peer allows the peer to use a new connection ID on
//# migration, as the peer will be unable to respond if the pool is
//# exhausted.
/// Limit on the number of connection IDs issued to the peer to reduce the amount
/// of per-path state maintained. Increasing this value allows peers to probe
/// more paths simultaneously at the expense of additional state to maintain.
const MAX_ACTIVE_CONNECTION_ID_LIMIT: u64 = 3;

/// Buffer to allow time for a peer to process and retire an expiring connection ID
/// before the connection ID actually expires.
///
/// When a local connection ID is retired a NEW_CONNECTION_ID frame is sent to the peer
/// containing a new connection ID to use as well as an increased "retire prior to" value.
/// The peer is required to send back a RETIRE_CONNECTION_ID frame retiring the connection ID(s)
/// indicated by the "retire prior to" value. When the RETIRE_CONNECTION_ID frame is
/// received, the connection ID is removed from use. The `EXPIRATION_BUFFER` is meant to
/// ensure the peer has enough time to cease using the connection ID before it is permanently
/// removed. 30 seconds should be enough time, even in extremely slow networks, for the
/// peer to flush all packets created with the old connection ID and to receive and start using
/// the new connection ID, at the minimal cost of maintaining state of an extra connection ID for
/// a brief period. Setting this value too low increases the risk of a packet being received with
/// a removed connection ID, resulting in a stateless reset that terminates the connection.
///
/// The value is not dynamically based on RTT as this would introduce a moving retirement time
/// target that would complicate arming the expiration timer and determining which connection IDs
/// should be retired.
const EXPIRATION_BUFFER: Duration = Duration::from_secs(30);

/// When a RETIRE_CONNECTION_ID frame is received, we don't want to remove the connection ID
/// immediately, since some packets with the just retired ID may still be received due to packet
/// reordering. The `RTT_MULTIPLIER` is multiplied by the smoothed RTT to calculate a removal
/// time that gives sufficient time for reordered packets to be processed.
const RTT_MULTIPLIER: u32 = 3;

/// A registration at the [`ConnectionIdMapper`].
///
/// It allows to add and remove external QUIC Connection IDs which are mapped to
/// internal IDs.
#[derive(Debug)]
pub struct LocalIdRegistry {
    /// The internal connection ID for this registration
    internal_id: InternalConnectionId,
    /// The shared state between mapper and registration
    state: Arc<Mutex<ConnectionIdMapperState>>,
    /// The connection IDs which are currently registered at the ConnectionIdMapper
    registered_ids: RegisteredIds,
    /// The sequence number to use the next time a new connection ID is registered
    next_sequence_number: u32,
    /// The current sequence number below which all connection IDs are considered retired
    retire_prior_to: u32,
    /// The maximum number of connection IDs to give to the peer
    active_connection_id_limit: u8,
    /// Memoized query to track retiring and expired connection IDs
    next_expiration: Memo<Option<Timestamp>, RegisteredIds>,
    /// Memoized query to track if there is any ACK interest
    ack_interest: Memo<bool, RegisteredIds>,
    /// Memoized query to track if there is any transmission interest
    transmission_interest: Memo<transmission::Interest, RegisteredIds>,
    /// Memoized query to track the number of active CIDs
    active_id_count: Memo<u8, RegisteredIds>,
}

type RegisteredIds = SmallVec<[LocalIdInfo; NR_STATIC_REGISTRABLE_IDS]>;

#[derive(Debug)]
struct LocalIdInfo {
    id: connection::LocalId,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //# Each Connection ID has an associated sequence number to assist in
    //# detecting when NEW_CONNECTION_ID or RETIRE_CONNECTION_ID frames refer
    //# to the same value.
    sequence_number: u32,
    retirement_time: Option<Timestamp>,
    stateless_reset_token: stateless_reset::Token,
    status: LocalIdStatus,
}

impl LocalIdInfo {
    // The time the connection ID next needs to change status (either to
    // PendingRetirementConfirmation, or removal altogether)
    fn next_status_change_time(&self) -> Option<Timestamp> {
        self.removal_time().or(self.retirement_time)
    }

    // Returns true if the connection ID should be moved to PendingRemoval
    fn is_retire_ready(&self, timestamp: Timestamp) -> bool {
        !self.is_retired()
            && self.retirement_time.map_or(false, |retirement_time| {
                retirement_time.has_elapsed(timestamp)
            })
    }

    // Returns true if the connection ID has been retired and is pending removal
    fn is_retired(&self) -> bool {
        matches!(
            self.status,
            PendingRetirementConfirmation(_) | PendingRemoval(_)
        )
    }

    // Returns true if the connection ID should no longer be used
    fn is_expired(&self, timestamp: Timestamp) -> bool {
        self.removal_time()
            .map_or(false, |removal_time| removal_time.has_elapsed(timestamp))
    }

    // The time this connection ID should be removed
    #[inline]
    fn removal_time(&self) -> Option<Timestamp> {
        match self.status {
            PendingRetirementConfirmation(removal_time) => removal_time,
            PendingRemoval(removal_time) => Some(removal_time),
            _ => None,
        }
    }

    // Changes the status of the connection ID to PendingRetirementConfirmation with a removal
    // time incorporating the EXPIRATION_BUFFER
    fn retire(&mut self, timestamp: Option<Timestamp>) {
        debug_assert!(!self.is_retired());
        self.status = PendingRetirementConfirmation(timestamp.map(|time| time + EXPIRATION_BUFFER))
    }

    #[inline]
    fn transmission_interest(&self) -> transmission::Interest {
        match self.status {
            PendingIssuance => transmission::Interest::NewData,
            PendingReissue => transmission::Interest::LostData,
            _ => transmission::Interest::None,
        }
    }

    /// Returns true if this status counts towards the active_connection_id_limit
    #[inline]
    fn counts_towards_limit(&self) -> bool {
        !matches!(
            self.status,
            PendingRetirementConfirmation(_) | PendingRemoval(_)
        )
    }
}

/// The current status of the connection ID.
#[derive(Debug, PartialEq, Eq)]
pub enum LocalIdStatus {
    /// New Connection IDs are put in the `PendingIssuance` status
    /// upon creation until a NEW_CONNECTION_ID frame has been sent
    /// to the peer to communicate the new connection ID.
    PendingIssuance,
    /// If the packet containing the NEW_CONNECTION_ID is lost, the
    /// connection ID is put into PendingReissue status.
    PendingReissue,
    /// Once a NEW_CONNECTION_ID frame is transmitted, the connection
    /// ID waits for acknowledgement of the packet.
    PendingAcknowledgement(PacketNumber),
    /// Once a connection ID has been communicated to the peer  and the
    /// peer has acknowledged the ID, it enters the `Active` status.
    /// The initial connection ID starts in this status.
    Active,
    /// Connection IDs are put in the `PendingRetirementConfirmation` status
    /// upon retirement, until confirmation of the retirement
    /// is received from the peer. If the optional removal_time indicated in this status
    /// is exceeded, the connection ID will be removed without confirmation from
    /// the peer.
    PendingRetirementConfirmation(Option<Timestamp>),
    /// Connection IDs are put in the `PendingRemoval` status
    /// when the peer has confirmed the retirement by sending a
    /// RETIRE_CONNECTION_ID_FRAME. This status exists to allow for a brief
    /// period before the Id is removed to account for packet reordering.
    PendingRemoval(Timestamp),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LocalIdRegistrationError {
    /// The Connection ID had already been registered
    ConnectionIdInUse,
    /// An invalid sequence number was specified
    InvalidSequenceNumber,
}

impl LocalIdRegistrationError {
    pub fn message(&self) -> &'static str {
        match self {
            LocalIdRegistrationError::ConnectionIdInUse => "Connection ID already in use",
            LocalIdRegistrationError::InvalidSequenceNumber => "Invalid sequence number",
        }
    }
}

impl Drop for LocalIdRegistry {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            // Unregister all previously registered IDs
            for id_info in &self.registered_ids {
                guard.local_id_map.remove(&id_info.id);
            }

            // Also clean up the initial ID if it had not already been removed
            guard.initial_id_map.remove(&self.internal_id);
        }
    }
}

impl LocalIdRegistry {
    /// Constructs a new `LocalIdRegistry` and registers the provided `handshake_connection_id`
    pub(crate) fn new(
        internal_id: InternalConnectionId,
        state: Arc<Mutex<ConnectionIdMapperState>>,
        handshake_connection_id: &connection::LocalId,
        handshake_connection_id_expiration_time: Option<Timestamp>,
        stateless_reset_token: stateless_reset::Token,
    ) -> Self {
        let mut registry = Self {
            internal_id,
            state,
            registered_ids: SmallVec::new(),
            next_sequence_number: 0,
            retire_prior_to: 0,
            // Initialize to 1 until we know the actual limit
            // from the peer transport parameters
            active_connection_id_limit: 1,
            next_expiration: Memo::new(|ids| {
                ids.iter()
                    .filter_map(|id_info| id_info.next_status_change_time())
                    .min()
            }),
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
            active_id_count: Memo::new(|ids| {
                let mut count = 0;
                for id in ids {
                    if id.counts_towards_limit() {
                        count += 1;
                    }
                }
                count
            }),
        };

        let _ = registry.register_connection_id(
            handshake_connection_id,
            handshake_connection_id_expiration_time,
            stateless_reset_token,
        );

        let handshake_connection_id_info = registry
            .registered_ids
            .iter_mut()
            .next()
            .expect("initial id added above");

        // The handshake connection ID is sent in the Initial packet,
        // so it starts in the `Active` status.
        handshake_connection_id_info.status = Active;
        registry.transmission_interest.clear();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# The sequence number of the initial connection ID is 0.
        debug_assert_eq!(handshake_connection_id_info.sequence_number, 0);

        registry.check_consistency();

        registry
    }

    /// Returns the associated internal connection ID
    pub fn internal_connection_id(&self) -> InternalConnectionId {
        self.internal_id
    }

    /// Sets the active connection id limit
    pub fn set_active_connection_id_limit(&mut self, active_connection_id_limit: u64) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# An endpoint MAY also limit the issuance of
        //# connection IDs to reduce the amount of per-path state it maintains,
        //# such as path validation status, as its peer might interact with it
        //# over as many paths as there are issued connection IDs.
        self.active_connection_id_limit =
            MAX_ACTIVE_CONNECTION_ID_LIMIT.min(active_connection_id_limit) as u8;
    }

    /// Registers a connection ID mapping at the mapper with an optional expiration
    /// timestamp. Returns the sequence number of the connection ID.
    ///
    /// This will return an error if the provided ConnectionId has already been
    /// registered or is already used by a different internal connection.
    pub fn register_connection_id(
        &mut self,
        id: &connection::LocalId,
        expiration: Option<Timestamp>,
        stateless_reset_token: stateless_reset::Token,
    ) -> Result<(), LocalIdRegistrationError> {
        if self.registered_ids.iter().any(|id_info| id_info.id == *id) {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1
            //# As a trivial example, this means the same connection ID
            //# MUST NOT be issued more than once on the same connection.
            return Err(LocalIdRegistrationError::ConnectionIdInUse);
        }

        self.validate_new_connection_id(stateless_reset_token);

        // Try to insert into the global map
        self.state
            .lock()
            .expect("should succeed unless the lock is poisoned")
            .local_id_map
            .try_insert(id, self.internal_id)
            .map_err(|_| LocalIdRegistrationError::ConnectionIdInUse)?;

        let sequence_number = self.next_sequence_number;
        let retirement_time = expiration.map(|expiration| expiration - EXPIRATION_BUFFER);

        // Track the inserted connection ID info
        self.registered_ids.push(LocalIdInfo {
            id: *id,
            sequence_number,
            retirement_time,
            stateless_reset_token,
            status: PendingIssuance,
        });
        self.active_id_count.clear();
        self.transmission_interest.clear();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# The sequence number on
        //# each newly issued connection ID MUST increase by 1.
        self.next_sequence_number += 1;

        // If we are provided an expiration, update the timers
        if expiration.is_some() {
            self.next_expiration.clear();
        }

        self.check_consistency();

        Ok(())
    }

    /// Unregisters connection IDs that have expired
    fn unregister_expired_ids(&mut self, timestamp: Timestamp) {
        {
            let mut mapper_state = self
                .state
                .lock()
                .expect("should succeed unless the lock is poisoned");

            self.registered_ids.retain(|id_info| {
                if id_info.is_expired(timestamp) {
                    let remove_result = mapper_state.local_id_map.remove(&id_info.id);
                    debug_assert!(
                        remove_result.is_some(),
                        "Connection ID should have been stored in mapper"
                    );

                    // clear all of the memoized values
                    self.ack_interest.clear();
                    self.transmission_interest.clear();
                    self.active_id_count.clear();
                    self.next_expiration.clear();

                    false // Don't retain
                } else {
                    true // Retain
                }
            });
        }

        self.check_consistency();
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //# When an endpoint issues a connection ID, it MUST accept packets that
    //# carry this connection ID for the duration of the connection or until
    //# its peer invalidates the connection ID via a RETIRE_CONNECTION_ID
    //# frame (Section 19.16).

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
    //# The endpoint SHOULD continue to
    //# accept the previously issued connection IDs until they are retired by
    //# the peer.
    /// Handles the retirement of a sequence_number received from a RETIRE_CONNECTION_ID frame
    pub fn on_retire_connection_id(
        &mut self,
        sequence_number: u32,
        destination_connection_id: &connection::LocalId,
        rtt: Duration,
        timestamp: Timestamp,
    ) -> Result<(), LocalIdRegistrationError> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
        //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
        //# greater than any previously sent to the peer MUST be treated as a
        //# connection error of type PROTOCOL_VIOLATION.
        if sequence_number >= self.next_sequence_number {
            return Err(LocalIdRegistrationError::InvalidSequenceNumber);
        }

        let id_info = self
            .registered_ids
            .iter_mut()
            // Filter out IDs that are already PendingRemoval, indicating this was a duplicate
            // RETIRE_CONNECTION_ID frame
            .filter(|id_info| !matches!(id_info.status, PendingRemoval(_)))
            .find(|id_info| id_info.sequence_number == sequence_number);

        if let Some(id_info) = id_info {
            if id_info.id == *destination_connection_id {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
                //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
                //# NOT refer to the Destination Connection ID field of the packet in
                //# which the frame is contained.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
                //# The peer MAY treat this as a
                //# connection error of type PROTOCOL_VIOLATION.
                return Err(LocalIdRegistrationError::InvalidSequenceNumber);
            }

            // Calculate a removal time based on RTT to give sufficient time for out of
            // order packets using the retired connection ID to be received
            let removal_time = timestamp + rtt * RTT_MULTIPLIER;

            id_info.status = PendingRemoval(removal_time);

            // clear all of the memoized values
            self.ack_interest.clear();
            self.transmission_interest.clear();
            self.active_id_count.clear();
            self.next_expiration.clear();
        }

        self.check_consistency();

        Ok(())
    }

    /// Returns the mappers interest in new connection IDs
    #[inline]
    pub fn connection_id_interest(&self) -> connection::id::Interest {
        let active_connection_id_count = self.active_id_count.get(&self.registered_ids);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# An endpoint SHOULD ensure that its peer has a sufficient number of
        //# available and unused connection IDs.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# An endpoint MUST NOT
        //# provide more connection IDs than the peer's limit.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# An endpoint SHOULD supply a new connection ID when the peer retires a
        //# connection ID.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.5
        //# To ensure that migration is possible and packets sent on different
        //# paths cannot be correlated, endpoints SHOULD provide new connection
        //# IDs before peers migrate; see Section 5.1.1.
        let new_connection_id_count = self.active_connection_id_limit - active_connection_id_count;

        if new_connection_id_count > 0 {
            self.check_active_connection_id_limit(
                active_connection_id_count,
                new_connection_id_count,
            );
            connection::id::Interest::New(new_connection_id_count)
        } else {
            connection::id::Interest::None
        }
    }

    /// Handles timeouts on the registration
    ///
    /// `timestamp` passes the current time.
    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if self.timer().poll_expiration(timestamp).is_ready() {
            for id_info in self
                .registered_ids
                .iter_mut()
                .filter(|id_info| id_info.is_retire_ready(timestamp))
            {
                id_info.retire(Some(timestamp));
                self.retire_prior_to = self.retire_prior_to.max(id_info.sequence_number + 1);

                // clear all of the memoized values
                self.ack_interest.clear();
                self.transmission_interest.clear();
                self.active_id_count.clear();
                self.next_expiration.clear();
            }

            self.unregister_expired_ids(timestamp);
        }

        self.check_consistency();
    }

    /// Writes any NEW_CONNECTION_ID frames necessary to the given context
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
            if let Some(packet_number) = context.write_frame(&frame::NewConnectionId {
                sequence_number: id_info.sequence_number.into(),
                retire_prior_to: self.retire_prior_to.into(),
                connection_id: id_info.id.as_bytes(),
                stateless_reset_token: id_info
                    .stateless_reset_token
                    .as_ref()
                    .try_into()
                    .expect("Length is already checked"),
            }) {
                id_info.status = PendingAcknowledgement(packet_number);
                self.transmission_interest.clear();
                self.ack_interest.clear();
            }
        }

        self.check_consistency();
    }

    /// Activates connection IDs that were pending acknowledgement
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        if !self.ack_interest.get(&self.registered_ids) {
            return;
        }

        for id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = Active;
                    // Once the NEW_CONNECTION_ID is acknowledged, we don't need the
                    // stateless reset token anymore.
                    id_info.stateless_reset_token = stateless_reset::Token::ZEROED;

                    self.ack_interest.clear();
                }
            }
        }

        self.check_consistency();
    }

    /// Moves connection IDs pending acknowledgement into pending reissue
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if !self.ack_interest.get(&self.registered_ids) {
            return;
        }

        for id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = PendingReissue;

                    self.ack_interest.clear();
                    self.transmission_interest.clear();
                }
            }
        }

        self.check_consistency();
    }

    /// Requests the peer to retire the connection id used during the handshake
    pub fn retire_handshake_connection_id(&mut self) {
        if let Some(handshake_id_info) = self
            .registered_ids
            .iter_mut()
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
            //# The sequence number of the initial connection ID is 0.
            .find(|id_info| id_info.sequence_number == 0 && !id_info.is_retired())
        {
            // Request the peer to retire the handshake CID immediately by incrementing retire_prior_to,
            // but schedule the removal of the handshake CID for its regularly scheduled retirement
            // time, since some peers may not be capable of retiring the handshake CID. This allows the
            // handshake CID to remain in use until the peer is ready to retire it or the handshake CID
            // expires, whichever comes first. If the handshake CID does not have a retirement time,
            // it will not be removed unless the peer sends a RETIRE_CONNECTION_ID frame.
            handshake_id_info.retire(handshake_id_info.retirement_time);

            // The retire_prior_to number is incremented to trigger the peer to send a
            // RETIRE_CONNECTION_ID frame if possible.
            self.retire_prior_to = self
                .retire_prior_to
                .max(handshake_id_info.sequence_number + 1);

            self.active_id_count.clear();
            self.next_expiration.clear();
            self.transmission_interest.clear();
        }

        self.check_consistency();
    }

    /// Validate that the current expiration timer is based on the next status change time
    fn check_consistency(&self) {
        if cfg!(debug_assertions) {
            self.next_expiration.check_consistency(&self.registered_ids);
            self.ack_interest.check_consistency(&self.registered_ids);
            self.transmission_interest
                .check_consistency(&self.registered_ids);
            self.active_id_count.check_consistency(&self.registered_ids);
        }
    }

    fn check_active_connection_id_limit(&self, active_count: u8, new_count: u8) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
        //# An endpoint MAY
        //# send connection IDs that temporarily exceed a peer's limit if the
        //# NEW_CONNECTION_ID frame also requires the retirement of any excess,
        //# by including a sufficiently large value in the Retire Prior To field.
        if cfg!(debug_assertions) {
            let retired_count = self
                .registered_ids
                .iter()
                .filter(|id_info| id_info.sequence_number < self.retire_prior_to)
                .count() as u8;
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
            //# An endpoint MAY
            //# send connection IDs that temporarily exceed a peer's limit if the
            //# NEW_CONNECTION_ID frame also requires the retirement of any excess,
            //# by including a sufficiently large value in the Retire Prior To field.
            assert!(
                (active_count + new_count).saturating_sub(retired_count)
                    <= self.active_connection_id_limit
            );
        }
    }

    #[inline]
    fn timer(&self) -> Timer {
        Timer::from(self.next_expiration.get(&self.registered_ids))
    }

    fn validate_new_connection_id(&self, new_token: stateless_reset::Token) {
        if cfg!(debug_assertions) {
            let active_count = self.active_id_count.get(&self.registered_ids);
            assert!(
                active_count < self.active_connection_id_limit,
                "Attempted to register more connection IDs than the active connection id limit: {}",
                self.active_connection_id_limit
            );

            assert!(
                !self
                    .registered_ids
                    .iter()
                    .map(|id_info| id_info.stateless_reset_token)
                    .any(|token| token == new_token),
                "Registered a duplicate stateless reset token"
            );
        }
    }
}

impl timer::Provider for LocalIdRegistry {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        query.on_timer(&self.timer())?;
        Ok(())
    }
}

impl transmission::interest::Provider for LocalIdRegistry {
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
