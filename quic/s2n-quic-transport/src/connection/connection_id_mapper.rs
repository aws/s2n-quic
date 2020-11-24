//! Maps from external connection IDs to internal connection IDs

use crate::connection::InternalConnectionId;
use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::{connection, time::Timestamp};
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
    /// connection to modify the mappings of it's Connection ID aliases.
    pub fn create_registration(
        &mut self,
        internal_id: InternalConnectionId,
    ) -> ConnectionIdMapperRegistration {
        ConnectionIdMapperRegistration {
            internal_id,
            state: self.state.clone(),
            registered_ids: SmallVec::new(),
            next_sequence_number: 0,
        }
    }
}

/// The amount of ConnectionIds we can register without dynamic memory allocation
const NR_STATIC_REGISTRABLE_IDS: usize = 5;

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
    next_sequence_number: u32,
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

    /// Registers a connection ID mapping at the mapper with an optional expiration
    /// timestamp. Returns the sequence number of the connection ID.
    ///
    /// This will return an error if the provided ConnectionId has already been
    /// registered or is already used by a different internal connection.
    pub fn register_connection_id(
        &mut self,
        id: &connection::Id,
        expiration: Option<Timestamp>,
    ) -> Result<u32, ConnectionIdMapperRegistrationError> {
        if self.registered_ids.iter().any(|id_info| id_info.id == *id) {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
            //# As a trivial example, this means the same connection ID
            //# MUST NOT be issued more than once on the same connection.
            return Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse);
        }

        // TODO: We might want to limit the maximum amount of aliases which
        // can be registered. But maybe there is already an implicit limit due to
        // how we provide IDs to the peer.

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
            });
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1.1
            //# The sequence number on
            //# each newly issued connection ID MUST increase by 1.
            self.next_sequence_number += 1;
            Ok(sequence_number)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::InternalConnectionIdGenerator;

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
    //= type=test
    //# As a trivial example, this means the same connection ID
    //# MUST NOT be issued more than once on the same connection.
    #[test]
    fn same_connection_id_must_not_be_issued_for_same_connection() {
        let mut reg = ConnectionIdMapper::new()
            .create_registration(InternalConnectionIdGenerator::new().generate_id());

        let ext_id = connection::Id::try_from_bytes(b"id1").unwrap();

        assert!(reg.register_connection_id(&ext_id, None).is_ok());
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
        let mut reg = ConnectionIdMapper::new()
            .create_registration(InternalConnectionIdGenerator::new().generate_id());

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();

        let seq_num_1 = reg.register_connection_id(&ext_id_1, None).unwrap();
        let seq_num_2 = reg.register_connection_id(&ext_id_2, None).unwrap();

        assert_eq!(1, seq_num_2 - seq_num_1);
    }

    #[test]
    fn connection_mapper_test() {
        let mut id_generator = InternalConnectionIdGenerator::new();
        let mut mapper = ConnectionIdMapper::new();

        let id1 = id_generator.generate_id();
        let id2 = id_generator.generate_id();

        let mut reg1 = mapper.create_registration(id1);
        let mut reg2 = mapper.create_registration(id2);

        let ext_id_1 = connection::Id::try_from_bytes(b"id1").unwrap();
        let ext_id_2 = connection::Id::try_from_bytes(b"id2").unwrap();
        let ext_id_3 = connection::Id::try_from_bytes(b"id3").unwrap();
        let ext_id_4 = connection::Id::try_from_bytes(b"id4").unwrap();

        let exp_1 = s2n_quic_platform::time::now();

        assert!(mapper.lookup_internal_connection_id(&ext_id_1).is_none());
        let result = reg1.register_connection_id(&ext_id_1, Some(exp_1));
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            reg1.registered_ids.get(0).unwrap().sequence_number
        );
        assert_eq!(Some(exp_1), reg1.registered_ids.get(0).unwrap().expiration);
        assert_eq!(Some(id1), mapper.lookup_internal_connection_id(&ext_id_1));

        assert_eq!(
            Err(ConnectionIdMapperRegistrationError::ConnectionIdInUse),
            reg2.register_connection_id(&ext_id_1, None)
        );

        assert!(reg1.register_connection_id(&ext_id_2, None).is_ok());
        assert!(reg2.register_connection_id(&ext_id_3, None).is_ok());
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
}
