use alloc::rc::Rc;
use core::{cell::RefCell, convert::TryInto};
use smallvec::SmallVec;

use s2n_quic_core::{
    ack_set::AckSet,
    connection, frame,
    packet::number::PacketNumber,
    stateless_reset,
    time::{Duration, Timer, Timestamp},
    transmission,
};

use crate::{
    connection::{connection_id_mapper::ConnectionIdMapperState, InternalConnectionId},
    contexts::WriteContext,
};

use crate::{connection::local_id_registry::LocalIdStatus::*, timer::VirtualTimer};

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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
    state: Rc<RefCell<ConnectionIdMapperState>>,
    /// The connection IDs which are currently registered at the ConnectionIdMapper
    registered_ids: SmallVec<[LocalIdInfo; NR_STATIC_REGISTRABLE_IDS]>,
    /// The sequence number to use the next time a new connection ID is registered
    next_sequence_number: u32,
    /// The current sequence number below which all connection IDs are considered retired
    retire_prior_to: u32,
    /// The maximum number of connection IDs to give to the peer
    active_connection_id_limit: u8,
    /// Timer set to track retiring and expired connection IDs
    expiration_timer: Timer,
}

#[derive(Debug)]
struct LocalIdInfo {
    id: connection::LocalId,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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
            && self
                .retirement_time
                .map_or(false, |retirement_time| retirement_time <= timestamp)
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
            .map_or(false, |removal_time| removal_time <= timestamp)
    }

    // The time this connection ID should be removed
    fn removal_time(&self) -> Option<Timestamp> {
        match self.status {
            PendingRetirementConfirmation(removal_time) => Some(removal_time),
            PendingRemoval(removal_time) => Some(removal_time),
            _ => None,
        }
    }

    // Changes the status of the connection ID to PendingRemoval with a removal
    // time incorporating the EXPIRATION_BUFFER
    fn retire(&mut self, timestamp: Timestamp) {
        debug_assert!(!self.is_retired());
        self.status = PendingRetirementConfirmation(timestamp + EXPIRATION_BUFFER)
    }
}

/// The current status of the connection ID.
#[derive(Debug, PartialEq)]
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
    /// is received from the peer. If the removal_time indicated in this status
    /// is exceeded, the connection ID will be removed without confirmation from
    /// the peer.
    PendingRetirementConfirmation(Timestamp),
    /// Connection IDs are put in the `PendingRemoval` status
    /// when the peer has confirmed the retirement by sending a
    /// RETIRE_CONNECTION_ID_FRAME. This status exists to allow for a brief
    /// period before the Id is removed to account for packet reordering.
    PendingRemoval(Timestamp),
}

impl LocalIdStatus {
    /// Returns true if this status counts towards the active_connection_id_limit
    fn counts_towards_limit(&self) -> bool {
        !matches!(self, PendingRetirementConfirmation(_) | PendingRemoval(_))
    }

    /// Returns true if this status allows for transmission based on the transmission constraint
    fn can_transmit(&self, constraint: transmission::Constraint) -> bool {
        match self {
            PendingReissue => constraint.can_retransmit(),
            PendingIssuance => constraint.can_transmit(),
            _ => false,
        }
    }
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
        let mut guard = self.state.borrow_mut();

        // Unregister all previously registered IDs
        for id_info in &self.registered_ids {
            guard.local_id_map.remove(&id_info.id);
        }
    }
}

impl LocalIdRegistry {
    /// Constructs a new `LocalIdRegistry` and registers provided `initial_connection_id`
    pub(crate) fn new(
        internal_id: InternalConnectionId,
        state: Rc<RefCell<ConnectionIdMapperState>>,
        initial_connection_id: &connection::LocalId,
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
            expiration_timer: VirtualTimer::default(),
        };

        // The initial connection ID will be retired after the handshake has completed
        // so an explicit expiration timestamp is not needed.
        let _ = registry.register_connection_id(initial_connection_id, None, stateless_reset_token);

        let initial_connection_id_info = registry
            .registered_ids
            .iter_mut()
            .next()
            .expect("initial id added above");

        // The initial connection ID is sent in the Initial packet,
        // so it starts in the `Active` status.
        initial_connection_id_info.status = Active;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# The sequence number of the initial connection ID is 0.
        debug_assert_eq!(initial_connection_id_info.sequence_number, 0);

        registry
    }

    /// Returns the associated internal connection ID
    pub fn internal_connection_id(&self) -> InternalConnectionId {
        self.internal_id
    }

    /// Sets the active connection id limit
    pub fn set_active_connection_id_limit(&mut self, active_connection_id_limit: u64) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
            //# As a trivial example, this means the same connection ID
            //# MUST NOT be issued more than once on the same connection.
            return Err(LocalIdRegistrationError::ConnectionIdInUse);
        }

        self.validate_new_connection_id(stateless_reset_token);

        // Try to insert into the global map
        if self
            .state
            .borrow_mut()
            .local_id_map
            .try_insert(id, self.internal_id)
            .is_ok()
        {
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
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# The sequence number on
            //# each newly issued connection ID MUST increase by 1.
            self.next_sequence_number += 1;

            // If we are provided an expiration, update the timers
            if expiration.is_some() {
                self.update_timers();
            }

            Ok(())
        } else {
            Err(LocalIdRegistrationError::ConnectionIdInUse)
        }
    }

    /// Unregisters connection IDs that have expired
    fn unregister_expired_ids(&mut self, timestamp: Timestamp) {
        {
            let mut mapper_state = self.state.borrow_mut();

            self.registered_ids.retain(|id_info| {
                if id_info.is_expired(timestamp) {
                    let remove_result = mapper_state.local_id_map.remove(&id_info.id);
                    debug_assert!(
                        remove_result.is_some(),
                        "Connection ID should have been stored in mapper"
                    );
                    false // Don't retain
                } else {
                    true // Retain
                }
            });
        }

        // Update the timers since we may have just removed the next retiring or expired id
        self.update_timers()
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //# When an endpoint issues a connection ID, it MUST accept packets that
    //# carry this connection ID for the duration of the connection or until
    //# its peer invalidates the connection ID via a RETIRE_CONNECTION_ID
    //# frame (Section 19.16).

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
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
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
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

        if let Some(mut id_info) = id_info {
            if id_info.id == *destination_connection_id {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
                //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
                //# NOT refer to the Destination Connection ID field of the packet in
                //# which the frame is contained.

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
                //# The peer MAY treat this as a
                //# connection error of type PROTOCOL_VIOLATION.
                return Err(LocalIdRegistrationError::InvalidSequenceNumber);
            }

            // Calculate a removal time based on RTT to give sufficient time for out of
            // order packets using the retired connection ID to be received
            let removal_time = timestamp + rtt * RTT_MULTIPLIER;

            id_info.status = PendingRemoval(removal_time);
            self.update_timers();
        }

        Ok(())
    }

    /// Returns the mappers interest in new connection IDs
    pub fn connection_id_interest(&self) -> connection::id::Interest {
        let active_connection_id_count = self
            .registered_ids
            .iter()
            .filter(|id_info| id_info.status.counts_towards_limit())
            .count() as u8;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# An endpoint SHOULD ensure that its peer has a sufficient number of
        //# available and unused connection IDs.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# An endpoint MUST NOT
        //# provide more connection IDs than the peer's limit.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# An endpoint SHOULD supply a new connection ID when the peer retires a
        //# connection ID.
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

    /// Gets the timers for the registration
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.check_timer_integrity();
        self.expiration_timer.iter()
    }

    /// Handles timeouts on the registration
    ///
    /// `timestamp` passes the current time.
    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if self.expiration_timer.poll_expiration(timestamp).is_ready() {
            for id_info in self
                .registered_ids
                .iter_mut()
                .filter(|id_info| id_info.is_retire_ready(timestamp))
            {
                id_info.retire(timestamp);
                self.retire_prior_to = self.retire_prior_to.max(id_info.sequence_number + 1)
            }

            self.unregister_expired_ids(timestamp);
        }
    }

    /// Writes any NEW_CONNECTION_ID frames necessary to the given context
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let constraint = context.transmission_constraint();

        for mut id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| id_info.status.can_transmit(constraint))
        {
            if let Some(packet_number) = context.write_frame(&frame::NewConnectionID {
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
            }
        }
    }

    /// Activates connection IDs that were pending acknowledgement
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        for mut id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = Active;
                    // Once the NEW_CONNECTION_ID is acknowledged, we don't need the
                    // stateless reset token anymore.
                    id_info.stateless_reset_token = stateless_reset::Token::ZEROED;
                }
            }
        }
    }

    /// Moves connection IDs pending acknowledgement into pending reissue
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        for mut id_info in self.registered_ids.iter_mut() {
            if let PendingAcknowledgement(packet_number) = id_info.status {
                if ack_set.contains(packet_number) {
                    id_info.status = PendingReissue;
                }
            }
        }
    }

    /// Retires all registered connection IDs
    pub fn retire_all(&mut self, timestamp: Timestamp) {
        // Retiring the connection ID with the highest sequence
        // number retires all connection ids prior to it as well.
        for id_info in self
            .registered_ids
            .iter_mut()
            .filter(|id_info| !id_info.is_retired())
        {
            id_info.retire(timestamp);
            self.retire_prior_to = self.retire_prior_to.max(id_info.sequence_number + 1);
        }

        self.update_timers();
    }

    /// Updates the expiration timer based on the current registered connection IDs
    fn update_timers(&mut self) {
        if let Some(timestamp) = self.next_status_change_time() {
            self.expiration_timer.set(timestamp);
        } else {
            self.expiration_timer.cancel();
        }
    }

    /// Gets the next time any of the registered IDs are expected to change their status
    fn next_status_change_time(&self) -> Option<Timestamp> {
        self.registered_ids
            .iter()
            .filter_map(|id_info| id_info.next_status_change_time())
            .min()
    }

    /// Validate that the current expiration timer is based on the next status change time
    fn check_timer_integrity(&self) {
        if cfg!(debug_assertions) {
            assert_eq!(
                self.expiration_timer.iter().next().cloned(),
                self.next_status_change_time()
            );
        }
    }

    fn check_active_connection_id_limit(&self, active_count: u8, new_count: u8) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
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

    fn validate_new_connection_id(&self, new_token: stateless_reset::Token) {
        if cfg!(debug_assertions) {
            assert!(
                self.registered_ids
                    .iter()
                    .filter(|id_info| id_info.status.counts_towards_limit())
                    .count()
                    < self.active_connection_id_limit as usize,
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

impl crate::transmission::interest::Provider for LocalIdRegistry {
    fn transmission_interest(&self) -> crate::transmission::Interest {
        let has_ids_pending_reissue = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingReissue);

        if has_ids_pending_reissue {
            return crate::transmission::Interest::LostData;
        }

        let has_ids_pending_issuance = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingIssuance);

        if has_ids_pending_issuance {
            crate::transmission::Interest::NewData
        } else {
            crate::transmission::Interest::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::{
        connection,
        connection::id::MIN_LIFETIME,
        frame::{Frame, NewConnectionID},
        packet::number::PacketNumberRange,
        random,
        stateless_reset::token::testing::*,
        varint::VarInt,
    };

    use crate::{
        connection::{
            connection_id_mapper::*,
            local_id_registry::{
                LocalIdInfo, LocalIdRegistrationError, LocalIdRegistry, EXPIRATION_BUFFER,
                MAX_ACTIVE_CONNECTION_ID_LIMIT, RTT_MULTIPLIER,
            },
            InternalConnectionIdGenerator,
        },
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        endpoint, transmission,
        transmission::interest::Provider,
    };
    use core::time::Duration;

    impl LocalIdRegistry {
        fn get_connection_id_info(&self, id: &connection::LocalId) -> Option<&LocalIdInfo> {
            self.registered_ids.iter().find(|id_info| id_info.id == *id)
        }

        fn get_connection_id_info_mut(
            &mut self,
            id: &connection::LocalId,
        ) -> Option<&mut LocalIdInfo> {
            self.registered_ids
                .iter_mut()
                .find(|id_info| id_info.id == *id)
        }
    }

    // Helper function to easily generate a LocalId from bytes
    fn id(bytes: &[u8]) -> connection::LocalId {
        connection::LocalId::try_from_bytes(bytes).unwrap()
    }

    // Helper function to easily create a LocalIdRegistry and Mapper
    fn mapper(
        initial_id: connection::LocalId,
        token: stateless_reset::Token,
    ) -> (ConnectionIdMapper, LocalIdRegistry) {
        let mut random_generator = random::testing::Generator(123);

        let mut mapper = ConnectionIdMapper::new(&mut random_generator);
        let registry = mapper.create_local_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            &initial_id,
            token,
        );
        (mapper, registry)
    }

    // Verify that an expiration with the earliest possible time results in a valid retirement time
    #[test]
    fn minimum_lifetime() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let expiration = s2n_quic_platform::time::now() + MIN_LIFETIME;

        let (_mapper, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(3);
        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration), TEST_TOKEN_2)
            .is_ok());
        assert_eq!(
            Some(expiration - EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_2)
                .unwrap()
                .retirement_time
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
    //= type=test
    //# As a trivial example, this means the same connection ID
    //# MUST NOT be issued more than once on the same connection.
    #[test]
    fn same_connection_id_must_not_be_issued_for_same_connection() {
        let ext_id = id(b"id01");
        let (_, mut reg) = mapper(ext_id, TEST_TOKEN_1);

        assert_eq!(
            Err(LocalIdRegistrationError::ConnectionIdInUse),
            reg.register_connection_id(&ext_id, None, TEST_TOKEN_1)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# The sequence number on
    //# each newly issued connection ID MUST increase by 1.
    #[test]
    fn sequence_number_must_increase_by_one() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let (_, mut reg) = mapper(ext_id_1, TEST_TOKEN_1);
        reg.set_active_connection_id_limit(3);
        reg.register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .unwrap();

        let seq_num_1 = reg
            .get_connection_id_info(&ext_id_1)
            .unwrap()
            .sequence_number;
        let seq_num_2 = reg
            .get_connection_id_info(&ext_id_2)
            .unwrap()
            .sequence_number;

        assert_eq!(1, seq_num_2 - seq_num_1);
    }

    #[test]
    fn connection_mapper_test() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator);

        let id1 = id_generator.generate_id();
        let id2 = id_generator.generate_id();

        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");
        let ext_id_4 = id(b"id04");

        let mut reg1 = mapper.create_local_id_registry(id1, &ext_id_1, TEST_TOKEN_1);
        let mut reg2 = mapper.create_local_id_registry(id2, &ext_id_3, TEST_TOKEN_3);

        let now = s2n_quic_platform::time::now();

        reg1.set_active_connection_id_limit(3);
        reg2.set_active_connection_id_limit(3);

        assert_eq!(
            0,
            reg1.get_connection_id_info(&ext_id_1)
                .unwrap()
                .sequence_number
        );
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_1));
        assert_eq!(
            TEST_TOKEN_1,
            reg1.get_connection_id_info(&ext_id_1)
                .unwrap()
                .stateless_reset_token
        );

        assert_eq!(
            Err(LocalIdRegistrationError::ConnectionIdInUse),
            reg2.register_connection_id(&ext_id_1, None, TEST_TOKEN_1)
        );

        let exp_2 = now + Duration::from_secs(60);

        assert!(reg1
            .register_connection_id(&ext_id_2, Some(exp_2), TEST_TOKEN_2)
            .is_ok());
        assert_eq!(
            Some(exp_2 - EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_2)
                .unwrap()
                .retirement_time
        );
        assert!(reg2
            .register_connection_id(&ext_id_4, None, TEST_TOKEN_4)
            .is_ok());
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_2));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        // Unregister id 3 (sequence number 0)
        reg2.get_connection_id_info_mut(&ext_id_3).unwrap().status = PendingRemoval(now);
        reg2.unregister_expired_ids(now);
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        reg2.get_connection_id_info_mut(&ext_id_4).unwrap().status =
            PendingRetirementConfirmation(now);
        reg2.unregister_expired_ids(now);
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_4));

        // Put back ID3 and ID4 to test drop behavior
        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());
        assert!(reg2
            .register_connection_id(&ext_id_4, None, TEST_TOKEN_4)
            .is_ok());
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        // If a registration is dropped all entries are removed
        drop(reg1);
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_1));
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_2));
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));
    }

    #[test]
    fn on_retire_connection_id() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let now = s2n_quic_platform::time::now();
        let (mapper, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(2);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //= type=test
        //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
        //# greater than any previously sent to the peer MUST be treated as a
        //# connection error of type PROTOCOL_VIOLATION.
        assert_eq!(
            Some(LocalIdRegistrationError::InvalidSequenceNumber),
            reg1.on_retire_connection_id(1, &ext_id_1, Duration::default(), now)
                .err()
        );

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        let rtt = Duration::from_millis(500);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //= type=test
        //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
        //# NOT refer to the Destination Connection ID field of the packet in
        //# which the frame is contained.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //= type=test
        //# The peer MAY treat this as a
        //# connection error of type PROTOCOL_VIOLATION.
        assert_eq!(
            Some(LocalIdRegistrationError::InvalidSequenceNumber),
            reg1.on_retire_connection_id(1, &ext_id_2, Duration::default(), now)
                .err()
        );

        assert!(reg1.on_retire_connection_id(1, &ext_id_1, rtt, now).is_ok());

        assert_eq!(
            PendingRemoval(now + rtt * RTT_MULTIPLIER),
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );

        // ID 1 wasn't impacted by the request to retire ID 2
        assert_eq!(
            Active,
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //= type=test
        //# An endpoint SHOULD supply a new connection ID when the peer retires a
        //# connection ID.
        assert_eq!(
            connection::id::Interest::New(1),
            reg1.connection_id_interest()
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //= type=test
        //# When an endpoint issues a connection ID, it MUST accept packets that
        //# carry this connection ID for the duration of the connection or until
        //# its peer invalidates the connection ID via a RETIRE_CONNECTION_ID
        //# frame (Section 19.16).

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.2
        //= type=test
        //# The endpoint SHOULD continue to
        //# accept the previously issued connection IDs until they are retired by
        //# the peer.
        reg1.unregister_expired_ids(now + rtt * RTT_MULTIPLIER);
        assert!(mapper.lookup_internal_connection_id(&ext_id_2).is_none());
    }

    #[test]
    fn on_retire_connection_id_pending_removal() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let now = s2n_quic_platform::time::now();

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(2);

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        reg1.retire_all(now);

        assert_eq!(
            PendingRetirementConfirmation(now + EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );
        assert_eq!(
            PendingRetirementConfirmation(now + EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );

        let rtt = Duration::from_millis(500);

        assert!(reg1.on_retire_connection_id(1, &ext_id_1, rtt, now).is_ok());

        assert_eq!(
            PendingRetirementConfirmation(now + EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );
        // When the ON_RETIRE_CONNECTION_ID frame is received from the peer, the
        // removal time for the retired connection ID is updated
        assert_eq!(
            PendingRemoval(now + rtt * RTT_MULTIPLIER),
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint that initiates migration and requires non-zero-length
    //# connection IDs SHOULD ensure that the pool of connection IDs
    //# available to its peer allows the peer to use a new connection ID on
    //# migration, as the peer will be unable to respond if the pool is
    //# exhausted.

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint SHOULD ensure that its peer has a sufficient number of
    //# available and unused connection IDs.
    #[test]
    fn connection_id_interest() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        // Active connection ID limit starts at 1, so there is no interest initially
        assert_eq!(
            connection::id::Interest::None,
            reg1.connection_id_interest()
        );

        reg1.set_active_connection_id_limit(5);
        assert_eq!(
            MAX_ACTIVE_CONNECTION_ID_LIMIT,
            reg1.active_connection_id_limit as u64
        );

        assert_eq!(
            connection::id::Interest::New(reg1.active_connection_id_limit - 1),
            reg1.connection_id_interest()
        );

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        assert_eq!(
            connection::id::Interest::New(reg1.active_connection_id_limit - 2),
            reg1.connection_id_interest()
        );

        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());

        assert_eq!(
            connection::id::Interest::None,
            reg1.connection_id_interest()
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint MUST NOT
    //# provide more connection IDs than the peer's limit.
    #[test]
    #[should_panic]
    fn endpoint_must_not_provide_more_ids_than_peer_limit() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(2);

        assert_eq!(
            connection::id::Interest::New(1),
            reg1.connection_id_interest()
        );

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        // Panics because we are inserting more than the limit
        let _ = reg1.register_connection_id(&ext_id_3, None, TEST_TOKEN_3);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint MAY
    //# send connection IDs that temporarily exceed a peer's limit if the
    //# NEW_CONNECTION_ID frame also requires the retirement of any excess,
    //# by including a sufficiently large value in the Retire Prior To field.
    #[test]
    fn endpoint_may_exceed_limit_temporarily() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let now = s2n_quic_platform::time::now();

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(2);

        assert_eq!(
            connection::id::Interest::New(1),
            reg1.connection_id_interest()
        );

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());
        reg1.retire_all(now + EXPIRATION_BUFFER);

        // We can register another ID because the retire_prior_to field retires old IDs
        assert_eq!(
            connection::id::Interest::New(2),
            reg1.connection_id_interest()
        );
        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint MAY also limit the issuance of
    //# connection IDs to reduce the amount of per-path state it maintains,
    //# such as path validation status, as its peer might interact with it
    //# over as many paths as there are issued connection IDs.
    #[test]
    fn endpoint_may_limit_connection_ids() {
        let ext_id_1 = id(b"id01");
        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(100);

        assert_eq!(
            MAX_ACTIVE_CONNECTION_ID_LIMIT,
            reg1.active_connection_id_limit as u64
        );
    }

    #[test]
    fn on_transmit() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let now = s2n_quic_platform::time::now();

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(3);

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        assert_eq!(
            transmission::Interest::NewData,
            reg1.transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );
        reg1.on_transmit(&mut write_context);

        let expected_frame = Frame::NewConnectionID {
            0: NewConnectionID {
                sequence_number: VarInt::from_u32(1),
                retire_prior_to: VarInt::from_u32(0),
                connection_id: ext_id_2.as_bytes(),
                stateless_reset_token: TEST_TOKEN_2.as_ref().try_into().unwrap(),
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        // Retire everything
        reg1.retire_all(now);
        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());

        assert_eq!(
            transmission::Interest::NewData,
            reg1.transmission_interest()
        );

        // Switch ID 3 to PendingReissue
        reg1.get_connection_id_info_mut(&ext_id_3).unwrap().status = PendingReissue;

        assert_eq!(
            transmission::Interest::LostData,
            reg1.transmission_interest()
        );

        reg1.on_transmit(&mut write_context);

        let expected_frame = Frame::NewConnectionID {
            0: NewConnectionID {
                sequence_number: VarInt::from_u32(2),
                retire_prior_to: VarInt::from_u32(2),
                connection_id: ext_id_3.as_bytes(),
                stateless_reset_token: TEST_TOKEN_3.as_ref().try_into().unwrap(),
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());
    }

    #[test]
    fn on_transmit_constrained() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(3);

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());
        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());

        assert_eq!(
            transmission::Interest::NewData,
            reg1.transmission_interest()
        );

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::RetransmissionOnly,
            endpoint::Type::Server,
        );
        reg1.on_transmit(&mut write_context);

        // No frame written because only retransmissions are allowed
        assert!(write_context.frame_buffer.is_empty());

        reg1.get_connection_id_info_mut(&ext_id_2).unwrap().status = PendingReissue;

        assert_eq!(
            transmission::Interest::LostData,
            reg1.transmission_interest()
        );

        reg1.on_transmit(&mut write_context);

        // Only the ID pending reissue should be written
        assert_eq!(1, write_context.frame_buffer.len());

        let expected_frame = Frame::NewConnectionID {
            0: NewConnectionID {
                sequence_number: VarInt::from_u32(1),
                retire_prior_to: VarInt::from_u32(0),
                connection_id: ext_id_2.as_bytes(),
                stateless_reset_token: TEST_TOKEN_2.as_ref().try_into().unwrap(),
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(
            transmission::Interest::NewData,
            reg1.transmission_interest()
        );
    }

    #[test]
    fn on_packet_ack_and_loss() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(3);

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );

        // Transition ID to PendingAcknowledgement
        let packet_number = write_context.packet_number();
        reg1.on_transmit(&mut write_context);

        // Packet was lost
        reg1.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            PendingReissue,
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );

        // Transition ID to PendingAcknowledgement again
        let packet_number = write_context.packet_number();
        reg1.on_transmit(&mut write_context);

        reg1.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

        assert_eq!(
            Active,
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );
        assert_eq!(
            stateless_reset::Token::ZEROED,
            reg1.get_connection_id_info(&ext_id_2)
                .unwrap()
                .stateless_reset_token
        );
    }

    #[test]
    fn timers() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(3);

        // No timer set for the initial connection ID
        assert_eq!(0, reg1.timers().count());

        let now = s2n_quic_platform::time::now();
        let expiration = now + Duration::from_secs(60);

        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration), TEST_TOKEN_2)
            .is_ok());

        // Expiration timer is armed based on retire time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(
            Some(expiration - EXPIRATION_BUFFER),
            reg1.timers().next().cloned()
        );

        reg1.get_connection_id_info_mut(&ext_id_1)
            .unwrap()
            .retire(now);
        reg1.update_timers();

        // Expiration timer is armed based on removal time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(
            Some(now + EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );

        reg1.get_connection_id_info_mut(&ext_id_2)
            .unwrap()
            .retire(now);
        reg1.update_timers();

        // Expiration timer is armed based on removal time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(Some(now + EXPIRATION_BUFFER), reg1.timers().next().cloned());

        // Unregister CIDs 1 and 2 (sequence numbers 0 and 1)
        reg1.unregister_expired_ids(now + Duration::from_secs(120));

        // No more timers are set
        assert_eq!(0, reg1.timers().count());
    }

    #[test]
    fn on_timeout() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);
        reg1.set_active_connection_id_limit(3);

        let now = s2n_quic_platform::time::now();

        // No timer set for the initial connection ID
        assert_eq!(0, reg1.timers().count());

        reg1.retire_all(now);

        // Too early, no timer is ready
        reg1.on_timeout(now);

        // Initial connection ID has an expiration set based on now
        assert_eq!(
            Some(now + EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );
        assert!(reg1.get_connection_id_info(&ext_id_1).is_some());

        // Now the expiration timer is ready
        reg1.on_timeout(now + EXPIRATION_BUFFER);
        // ID 1 was removed since it expired
        assert!(reg1.get_connection_id_info(&ext_id_1).is_none());
        assert!(!reg1.expiration_timer.is_armed());

        let expiration_2 = now + Duration::from_secs(60);
        let expiration_3 = now + Duration::from_secs(120);

        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration_2), TEST_TOKEN_2)
            .is_ok());
        assert!(reg1
            .register_connection_id(&ext_id_3, Some(expiration_3), TEST_TOKEN_3)
            .is_ok());

        // Expiration timer is set based on the retirement time of ID 2
        assert_eq!(
            Some(expiration_2 - EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );

        reg1.on_timeout(expiration_2 - EXPIRATION_BUFFER);

        // ID 2 is moved into pending retirement confirmation
        assert_eq!(
            PendingRetirementConfirmation(expiration_2),
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );
        // Expiration timer is set to the expiration time of ID 2
        assert_eq!(
            Some(expiration_2),
            reg1.expiration_timer.iter().next().cloned()
        );

        reg1.on_timeout(expiration_2);

        assert!(reg1.get_connection_id_info(&ext_id_2).is_none());

        // Expiration timer is set to the retirement time of ID 3
        assert_eq!(
            Some(expiration_3 - EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );
    }

    #[test]
    fn retire_all() {
        let ext_id_1 = id(b"id01");
        let ext_id_2 = id(b"id02");
        let ext_id_3 = id(b"id03");

        let (_, mut reg1) = mapper(ext_id_1, TEST_TOKEN_1);

        reg1.set_active_connection_id_limit(3);

        assert!(reg1
            .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
            .is_ok());
        assert!(reg1
            .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
            .is_ok());

        reg1.retire_all(s2n_quic_platform::time::now());

        assert_eq!(3, reg1.registered_ids.iter().count());

        for id_info in reg1.registered_ids.iter() {
            assert!(id_info.is_retired());
        }

        // Calling retire_all again does nothing
        reg1.retire_all(s2n_quic_platform::time::now());

        assert_eq!(3, reg1.registered_ids.iter().count());

        for id_info in reg1.registered_ids.iter() {
            assert!(id_info.is_retired());
        }
    }
}
