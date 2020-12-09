//! Maps from external connection IDs to internal connection IDs

use crate::{
    connection::{
        connection_id_mapper::LocalConnectionIdStatus::{
            Active, PendingAcknowledgement, PendingIssuance, PendingReissue, PendingRetirement,
        },
        InternalConnectionId,
    },
    timer::VirtualTimer,
    transmission::{self, WriteContext},
};
use alloc::rc::Rc;
use core::{cell::RefCell, time::Duration};
use s2n_quic_core::{
    ack_set::AckSet, connection, frame, packet::number::PacketNumber, time::Timestamp,
};
use smallvec::SmallVec;
use std::collections::hash_map::{Entry, HashMap};

#[derive(Debug)]
struct ConnectionIdMapperState {
    /// Maps from external to internal connection IDs
    connection_map: HashMap<connection::Id, InternalConnectionId>,
}

impl ConnectionIdMapperState {
    fn new() -> Self {
        Self {
            connection_map: HashMap::new(),
        }
    }

    fn try_insert(
        &mut self,
        external_id: &connection::Id,
        internal_id: InternalConnectionId,
    ) -> Result<(), ()> {
        let entry = self.connection_map.entry(*external_id);
        match entry {
            Entry::Occupied(_) => Err(()),
            Entry::Vacant(entry) => {
                entry.insert(internal_id);
                Ok(())
            }
        }
    }
}

/// Maps from external connection IDs to internal connection IDs
pub struct ConnectionIdMapper {
    /// The shared state between mapper and registration
    state: Rc<RefCell<ConnectionIdMapperState>>,
}

impl ConnectionIdMapper {
    /// Creates a new `ConnectionIdMapper`
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(ConnectionIdMapperState::new())),
        }
    }

    /// Looks up the internal Connection ID which is associated with an external
    /// connection ID.
    pub fn lookup_internal_connection_id(
        &self,
        connection_id: &connection::Id,
    ) -> Option<InternalConnectionId> {
        let guard = self.state.borrow();
        guard.connection_map.get(connection_id).map(Clone::clone)
    }

    /// Creates a registration for a new internal connection ID, which allows that
    /// connection to modify the mappings of it's Connection ID aliases. The provided
    /// `initial_connection_id` will be registered in the returned registration.
    pub fn create_registration(
        &mut self,
        internal_id: InternalConnectionId,
        initial_connection_id: &connection::Id,
    ) -> ConnectionIdMapperRegistration {
        let mut registration = ConnectionIdMapperRegistration {
            internal_id,
            state: self.state.clone(),
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
        let _ = registration.register_connection_id(initial_connection_id, None);

        let initial_connection_id_info = registration
            .get_connection_id_info_mut(&initial_connection_id)
            .expect("initial id added above");

        // The initial connection ID is sent in the Initial packet,
        // so it starts in the `Active` status.
        initial_connection_id_info.status = Active;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
        //# The sequence number of the initial connection ID is 0.
        debug_assert_eq!(initial_connection_id_info.sequence_number, 0);

        registration
    }
}

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

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

/// A registration at the [`ConnectionIdMapper`].
///
/// It allows to add and remove external QUIC Connection IDs which are mapped to
/// internal IDs.
#[derive(Debug)]
pub struct ConnectionIdMapperRegistration {
    /// The internal connection ID for this registration
    internal_id: InternalConnectionId,
    /// The shared state between mapper and registration
    state: Rc<RefCell<ConnectionIdMapperState>>,
    /// The connection IDs which are currently registered at the ConnectionIdMapper
    registered_ids: SmallVec<[LocalConnectionIdInfo; NR_STATIC_REGISTRABLE_IDS]>,
    /// The sequence number to use the next time a new connection ID is registered
    next_sequence_number: u32,
    /// The current sequence number below which all connection IDs are considered retired
    retire_prior_to: u32,
    /// The maximum number of connection IDs to give to the peer
    active_connection_id_limit: u8,
    /// Timer set to track retiring and expired connection IDs
    expiration_timer: VirtualTimer,
}

#[derive(Debug)]
struct LocalConnectionIdInfo {
    id: connection::Id,
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //# Each Connection ID has an associated sequence number to assist in
    //# detecting when NEW_CONNECTION_ID or RETIRE_CONNECTION_ID frames refer
    //# to the same value.
    sequence_number: u32,
    expiration: Option<Timestamp>,
    status: LocalConnectionIdStatus,
}

impl LocalConnectionIdInfo {
    // Gets the time at which the connection ID should be retired. This time is prior to the
    // expiration to account for the delay between locally retiring a connection ID and the peer
    // retiring the connection ID
    fn retirement_time(&self) -> Option<Timestamp> {
        self.expiration
            .map(|expiration| expiration - EXPIRATION_BUFFER)
    }

    // Gets the time at which the connection ID should no longer be in use
    fn expiration_time(&self) -> Option<Timestamp> {
        self.expiration
    }

    // The time the connection ID next needs to change status (either to
    // PENDING_RETIREMENT, or removal altogether)
    fn next_status_change_time(&self) -> Option<Timestamp> {
        if matches!(self.status, PendingRetirement) {
            self.expiration_time()
        } else {
            self.retirement_time()
        }
    }

    // Moves the connection ID to PendingRetirement and sets the expiration
    // if none was set already.
    fn retire(&mut self, timestamp: Timestamp) {
        debug_assert_ne!(self.status, PendingRetirement);

        self.status = PendingRetirement;

        // Set an expiration if the connection ID didn't already have one so we
        // are sure to clean it up if the peer doesn't retire it as expected. This
        // only impacts the initial connection ID, which is retired upon handshake
        // completion.
        if self.expiration.is_none() {
            self.expiration = Some(timestamp + EXPIRATION_BUFFER);
        }
    }

    // Returns true if the connection ID should be moved to PENDING_RETIREMENT
    fn is_retire_ready(&self, timestamp: Timestamp) -> bool {
        self.status != PendingRetirement
            && self
                .retirement_time()
                .map_or(false, |retirement_time| retirement_time <= timestamp)
    }

    // Returns true if the connection ID should no longer be used
    fn is_expired(&self, timestamp: Timestamp) -> bool {
        self.expiration_time()
            .map_or(false, |expiration_time| expiration_time <= timestamp)
    }
}

/// The current status of the connection ID.
#[derive(Debug, PartialEq)]
enum LocalConnectionIdStatus {
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
    /// Connection IDs are put in the `PendingRetirement` status
    /// upon retirement, until confirmation of the retirement
    /// is received from the peer.
    PendingRetirement,
}

impl LocalConnectionIdStatus {
    /// Returns true if this status counts towards the active_connection_id_limit
    fn counts_towards_limit(&self) -> bool {
        !matches!(self, PendingRetirement)
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
pub enum ConnectionIdMapperRegistrationError {
    /// The Connection ID had already been registered
    ConnectionIdInUse,
    /// An invalid sequence number was specified
    InvalidSequenceNumber,
}

impl Drop for ConnectionIdMapperRegistration {
    fn drop(&mut self) {
        let mut guard = self.state.borrow_mut();

        // Unregister all previously registered IDs
        for id_info in &self.registered_ids {
            guard.connection_map.remove(&id_info.id);
        }
    }
}

impl ConnectionIdMapperRegistration {
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
        id: &connection::Id,
        expiration: Option<Timestamp>,
    ) -> Result<(), ConnectionIdMapperRegistrationError> {
        if self.registered_ids.iter().any(|id_info| id_info.id == *id) {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
            //# As a trivial example, this means the same connection ID
            //# MUST NOT be issued more than once on the same connection.
            return Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse);
        }

        debug_assert!(
            self.registered_ids
                .iter()
                .filter(|id_info| id_info.status.counts_towards_limit())
                .count()
                < self.active_connection_id_limit as usize,
            "Attempted to register more connection IDs than the active connection id limit: {}",
            self.active_connection_id_limit
        );

        // Try to insert into the global map
        if self
            .state
            .borrow_mut()
            .try_insert(id, self.internal_id)
            .is_ok()
        {
            let sequence_number = self.next_sequence_number;

            // Track the inserted connection ID info
            self.registered_ids.push(LocalConnectionIdInfo {
                id: *id,
                sequence_number,
                expiration,
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
            Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse)
        }
    }

    /// Unregisters the connection ID with the given `sequence_number`
    pub fn unregister_connection_id(
        &mut self,
        sequence_number: u32,
    ) -> Result<(), ConnectionIdMapperRegistrationError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
        //# greater than any previously sent to the peer MUST be treated as a
        //# connection error of type PROTOCOL_VIOLATION.
        if sequence_number >= self.next_sequence_number {
            return Err(ConnectionIdMapperRegistrationError::InvalidSequenceNumber);
        }

        let registration_index = match self
            .registered_ids
            .iter()
            .position(|id_info| id_info.sequence_number == sequence_number)
        {
            Some(index) => index,
            None => return Ok(()), // Nothing to do
        };

        let removed_id_info = self.registered_ids.remove(registration_index);

        // Try to remove from the global map
        let remove_result = self
            .state
            .borrow_mut()
            .connection_map
            .remove(&removed_id_info.id);
        debug_assert!(
            remove_result.is_some(),
            "Connection ID should have been stored in mapper"
        );

        // Update the timers since we may have just removed the next retiring or expiring id
        self.update_timers();

        Ok(())
    }

    /// Moves all registered connection IDs with a sequence number less
    /// than or equal to the sequence number of the provided `connection::Id`
    /// into the `PendingRetirement` status.
    pub fn retire_connection_id(&mut self, id: &connection::Id, timestamp: Timestamp) {
        if let Some(retired_id_info) = self.get_connection_id_info(id) {
            debug_assert_ne!(retired_id_info.status, PendingRetirement);

            let retired_sequence_number = retired_id_info.sequence_number;
            self.registered_ids
                .iter_mut()
                .filter(|id_info| id_info.sequence_number <= retired_sequence_number)
                .filter(|id_info| id_info.status != PendingRetirement)
                .for_each(|id_info| id_info.retire(timestamp));
            self.retire_prior_to = self.retire_prior_to.max(retired_sequence_number + 1);

            self.update_timers();
        }
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
        let new_connection_id_count = self.active_connection_id_limit - active_connection_id_count;

        if new_connection_id_count > 0 {
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
            // We only need the latest retire ready connection ID since retiring that ID will
            // retire all earlier connection IDs as well
            let latest_retire_ready_id = self
                .registered_ids
                .iter()
                .filter(|id_info| id_info.is_retire_ready(timestamp))
                .max_by_key(|id_info| id_info.sequence_number)
                .map(|id_info| id_info.id);

            if let Some(id) = latest_retire_ready_id {
                self.retire_connection_id(&id, timestamp);
            }

            let expired_id_count = self
                .registered_ids
                .iter()
                .filter(|id_info| id_info.is_expired(timestamp))
                .count();

            if expired_id_count > 0 {
                // Generally there shouldn't be any IDs that are expired, as connection IDs will be
                // removed based on RETIRE_CONNECTION_ID frames received from the peer. If those
                // frames take longer than the EXPIRATION_BUFFER to receive for some reason, this
                // check ensures the connection IDs are still removed.
                let mut expired_sequence_numbers =
                    SmallVec::<[u32; MAX_ACTIVE_CONNECTION_ID_LIMIT as usize]>::new();

                self.registered_ids
                    .iter()
                    .filter(|id_info| id_info.is_expired(timestamp))
                    .for_each(|id_info| expired_sequence_numbers.push(id_info.sequence_number));

                for sequence_number in expired_sequence_numbers {
                    self.unregister_connection_id(sequence_number)
                        .expect("sequence number came from registered ids");
                }
            }
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
                stateless_reset_token: &[1; 16], // TODO https://github.com/awslabs/s2n-quic/issues/195
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
        if let Some(id) = self
            .registered_ids
            .iter()
            .filter(|id_info| id_info.status != PendingRetirement)
            .max_by_key(|id_info| id_info.sequence_number)
            .map(|id_info| id_info.id)
        {
            self.retire_connection_id(&id, timestamp)
        }
    }

    fn get_connection_id_info(&self, id: &connection::Id) -> Option<&LocalConnectionIdInfo> {
        self.registered_ids.iter().find(|id_info| id_info.id == *id)
    }

    fn get_connection_id_info_mut(
        &mut self,
        id: &connection::Id,
    ) -> Option<&mut LocalConnectionIdInfo> {
        self.registered_ids
            .iter_mut()
            .find(|id_info| id_info.id == *id)
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
}

impl transmission::interest::Provider for ConnectionIdMapperRegistration {
    fn transmission_interest(&self) -> transmission::Interest {
        let has_ids_pending_reissue = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingReissue);

        if has_ids_pending_reissue {
            return transmission::Interest::LostData;
        }

        let has_ids_pending_issuance = self
            .registered_ids
            .iter()
            .any(|id_info| id_info.status == PendingIssuance);

        if has_ids_pending_issuance {
            transmission::Interest::NewData
        } else {
            transmission::Interest::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::InternalConnectionIdGenerator,
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        endpoint,
        transmission::interest::Provider,
    };
    use s2n_quic_core::{
        connection::id::MIN_LIFETIME,
        frame::{Frame, NewConnectionID},
        packet::number::PacketNumberRange,
        varint::VarInt,
    };

    // Verify that an expiration with the earliest possible time results in a valid retirement time
    #[test]
    fn minimum_lifetime() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();

        let expiration = s2n_quic_platform::time::now() + MIN_LIFETIME;

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);
        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration))
            .is_ok());
        assert_eq!(
            Some(expiration - EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_2)
                .unwrap()
                .retirement_time()
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
    //= type=test
    //# As a trivial example, this means the same connection ID
    //# MUST NOT be issued more than once on the same connection.
    #[test]
    fn same_connection_id_must_not_be_issued_for_same_connection() {
        let ext_id = connection::Id::try_from_bytes(b"id1").unwrap();
        let mut reg = ConnectionIdMapper::new()
            .create_registration(InternalConnectionIdGenerator::new().generate_id(), &ext_id);

        assert_eq!(
            Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse),
            reg.register_connection_id(&ext_id, None)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# The sequence number on
    //# each newly issued connection ID MUST increase by 1.
    #[test]
    fn sequence_number_must_increase_by_one() {
        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();

        let mut reg = ConnectionIdMapper::new().create_registration(
            InternalConnectionIdGenerator::new().generate_id(),
            &ext_id_1,
        );
        reg.set_active_connection_id_limit(3);
        reg.register_connection_id(&ext_id_2, None).unwrap();

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
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();
        let id2 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();
        let ext_id_4 = connection::Id::try_from_bytes(b"id4").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        let mut reg2 = mapper.create_registration(id2, &ext_id_3);

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
            Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse),
            reg2.register_connection_id(&ext_id_1, None)
        );

        let exp_2 = s2n_quic_platform::time::now() + Duration::from_secs(60);

        assert!(reg1.register_connection_id(&ext_id_2, Some(exp_2)).is_ok());
        assert_eq!(
            Some(exp_2),
            reg1.get_connection_id_info(&ext_id_2).unwrap().expiration
        );
        assert!(reg2.register_connection_id(&ext_id_4, None).is_ok());
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_2));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        // Unregister id 3 (sequence number 0)
        assert!(reg2.unregister_connection_id(0).is_ok());
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_3));

        // If a registration is dropped all entries are removed
        drop(reg1);
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_1));
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_2));
        assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //= type=test
        //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
        //# greater than any previously sent to the peer MUST be treated as a
        //# connection error of type PROTOCOL_VIOLATION.
        assert_eq!(
            Some(ConnectionIdMapperRegistrationError::InvalidSequenceNumber),
            reg2.unregister_connection_id(2).err()
        );
    }

    #[test]
    fn retire_connection_id() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let now = s2n_quic_platform::time::now();
        let expiration = now + Duration::from_secs(60);

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert_eq!(0, reg1.retire_prior_to);
        // Retiring an unregistered ID does nothing
        reg1.retire_connection_id(&ext_id_2, now);
        assert_eq!(0, reg1.retire_prior_to);
        assert_eq!(
            Active,
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );

        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration))
            .is_ok());
        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

        // Retire ID 2 (sequence number 1)
        reg1.retire_connection_id(&ext_id_2, now);

        // ID 2 and all those before it should be retired
        assert_eq!(
            PendingRetirement,
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );
        assert_eq!(
            PendingRetirement,
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
        );
        // ID 1 didn't have an expiration so it should get one upon retirement
        assert_eq!(
            Some(now + EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_1).unwrap().expiration
        );
        assert_eq!(
            Some(expiration),
            reg1.get_connection_id_info(&ext_id_2).unwrap().expiration
        );

        assert_eq!(2, reg1.retire_prior_to);

        // ID 3 was after ID 2, so it is not retired
        assert_eq!(
            PendingIssuance,
            reg1.get_connection_id_info(&ext_id_3).unwrap().status
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint SHOULD ensure that its peer has a sufficient number of
    //# available and unused connection IDs.
    #[test]
    fn connection_id_interest() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);

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

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());

        assert_eq!(
            connection::id::Interest::New(reg1.active_connection_id_limit - 2),
            reg1.connection_id_interest()
        );

        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

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
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(2);

        assert_eq!(
            connection::id::Interest::New(1),
            reg1.connection_id_interest()
        );

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());

        // Panics because we are inserting more than the limit
        let _ = reg1.register_connection_id(&ext_id_3, None);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
    //= type=test
    //# An endpoint MAY also limit the issuance of
    //# connection IDs to reduce the amount of per-path state it maintains,
    //# such as path validation status, as its peer might interact with it
    //# over as many paths as there are issued connection IDs.
    #[test]
    fn endpoint_may_limit_connection_ids() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(100);

        assert_eq!(
            MAX_ACTIVE_CONNECTION_ID_LIMIT,
            reg1.active_connection_id_limit as u64
        );
    }

    #[test]
    fn on_transmit() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let now = s2n_quic_platform::time::now();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());

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
                stateless_reset_token: &[1; 16],
            },
        };

        assert_eq!(
            expected_frame,
            write_context.frame_buffer.pop_front().unwrap().as_frame()
        );

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        reg1.retire_connection_id(&ext_id_2, now);
        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

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
                stateless_reset_token: &[1; 16],
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
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert_eq!(transmission::Interest::None, reg1.transmission_interest());

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());
        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

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
                stateless_reset_token: &[1; 16],
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
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());

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
    }

    #[test]
    fn timers() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        // No timer set for the initial connection ID
        assert_eq!(0, reg1.timers().count());

        let now = s2n_quic_platform::time::now();
        let expiration = now + Duration::from_secs(60);

        assert!(reg1
            .register_connection_id(&ext_id_2, Some(expiration))
            .is_ok());

        // Expiration timer is armed based on retire time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(
            Some(expiration - EXPIRATION_BUFFER),
            reg1.timers().next().cloned()
        );

        reg1.retire_connection_id(&ext_id_1, now);

        // Expiration timer is armed based on expiration time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(
            Some(now + EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );

        reg1.retire_connection_id(&ext_id_2, now);

        // Expiration timer is armed based on expiration time
        assert_eq!(1, reg1.timers().count());
        assert_eq!(Some(now + EXPIRATION_BUFFER), reg1.timers().next().cloned());

        // Unregister CIDs 1 and 2 (sequence numbers 0 and 1)
        assert!(reg1.unregister_connection_id(0).is_ok());
        assert!(reg1.unregister_connection_id(1).is_ok());

        // No more timers are set
        assert_eq!(0, reg1.timers().count());
    }

    #[test]
    fn on_timeout() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        let now = s2n_quic_platform::time::now();

        // No timer set for the initial connection ID
        assert_eq!(0, reg1.timers().count());

        reg1.retire_connection_id(&ext_id_1, now);

        // Initial connection ID has an expiration set based on now
        assert_eq!(
            Some(now + EXPIRATION_BUFFER),
            reg1.get_connection_id_info(&ext_id_1)
                .unwrap()
                .expiration_time()
        );

        // Too early, no timer is ready
        reg1.on_timeout(now);

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
            .register_connection_id(&ext_id_2, Some(expiration_2))
            .is_ok());
        assert!(reg1
            .register_connection_id(&ext_id_3, Some(expiration_3))
            .is_ok());

        // Expiration timer is set based on the retirement time of ID 2
        assert_eq!(
            Some(expiration_2 - EXPIRATION_BUFFER),
            reg1.expiration_timer.iter().next().cloned()
        );

        reg1.on_timeout(expiration_2 - EXPIRATION_BUFFER);

        // ID 2 is moved into pending retirement
        assert_eq!(
            PendingRetirement,
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
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());
        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

        reg1.retire_connection_id(&ext_id_3, s2n_quic_platform::time::now());

        reg1.retire_all(s2n_quic_platform::time::now());

        assert_eq!(3, reg1.registered_ids.iter().count());

        for status in reg1.registered_ids.iter().map(|id_info| &id_info.status) {
            assert_eq!(PendingRetirement, *status);
        }

        // Calling retire_all again does nothing
        reg1.retire_all(s2n_quic_platform::time::now());

        assert_eq!(3, reg1.registered_ids.iter().count());

        for status in reg1.registered_ids.iter().map(|id_info| &id_info.status) {
            assert_eq!(PendingRetirement, *status);
        }
    }
}
