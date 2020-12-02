//! Maps from external connection IDs to internal connection IDs

use crate::{
    connection::{
        connection_id_mapper::LocalConnectionIdStatus::{
            Active, PendingAcknowledgement, PendingIssuance, PendingReissue, PendingRetirement,
        },
        InternalConnectionId,
    },
    transmission::{self, WriteContext},
};
use alloc::rc::Rc;
use core::cell::RefCell;
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
        match self {
            PendingRetirement => false,
            _ => true,
        }
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
            Ok(())
        } else {
            Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse)
        }
    }

    /// Unregisters a connection ID at the mapper
    pub fn unregister_connection_id(&mut self, id: &connection::Id) {
        let registration_index = match self
            .registered_ids
            .iter()
            .position(|id_info| id_info.id == *id)
        {
            Some(index) => index,
            None => return, // Nothing to do
        };

        // Try to insert into the global map
        let remove_result = self.state.borrow_mut().connection_map.remove(id);
        debug_assert!(
            remove_result.is_some(),
            "Connection ID should have been stored in mapper"
        );

        self.registered_ids.remove(registration_index);
    }

    /// Moves all registered connection IDs with a sequence number less
    /// than or equal to the sequence number of the provided `connection::Id`
    /// into the `PendingRetirement` status.
    pub fn retire_connection_id(&mut self, id: &connection::Id) {
        if let Some(retired_id_info) = self.get_connection_id_info(id) {
            let retired_sequence_number = retired_id_info.sequence_number;
            self.registered_ids
                .iter_mut()
                .filter(|id_info| id_info.sequence_number <= retired_sequence_number)
                .for_each(|mut id_info| id_info.status = PendingRetirement);
            self.retire_prior_to = self.retire_prior_to.max(retired_sequence_number + 1);
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
        frame::{Frame, NewConnectionID},
        packet::number::PacketNumberRange,
        varint::VarInt,
    };

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

        let exp_2 = s2n_quic_platform::time::now();

        assert!(reg1.register_connection_id(&ext_id_2, Some(exp_2)).is_ok());
        assert_eq!(
            Some(exp_2),
            reg1.get_connection_id_info(&ext_id_2).unwrap().expiration
        );
        assert!(reg2.register_connection_id(&ext_id_4, None).is_ok());
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_2));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_3));
        assert_eq!(Some(id2), mapper.lookup_internal_connection_id(&ext_id_4));

        reg2.unregister_connection_id(&ext_id_3);
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
    }

    #[test]
    fn retire_connection_id() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();

        let mut reg1 = mapper.create_registration(id1, &ext_id_1);
        reg1.set_active_connection_id_limit(3);

        assert_eq!(0, reg1.retire_prior_to);
        // Retiring an unregistered ID does nothing
        reg1.retire_connection_id(&ext_id_2);
        assert_eq!(0, reg1.retire_prior_to);
        assert_eq!(
            Active,
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());
        assert!(reg1.register_connection_id(&ext_id_3, None).is_ok());

        // Retire ID 2 (sequence number 1)
        reg1.retire_connection_id(&ext_id_2);

        // ID 3 and all those before it should be retired
        assert_eq!(
            PendingRetirement,
            reg1.get_connection_id_info(&ext_id_1).unwrap().status
        );
        assert_eq!(
            PendingRetirement,
            reg1.get_connection_id_info(&ext_id_2).unwrap().status
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

        reg1.retire_connection_id(&ext_id_2);
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
}
