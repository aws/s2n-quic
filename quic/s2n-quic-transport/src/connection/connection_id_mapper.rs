// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Maps from external connection IDs to internal connection IDs

use crate::connection::{local_id_registry::LocalIdRegistry, InternalConnectionId, PeerIdRegistry};
use core::{convert::TryFrom as _, hash::BuildHasher};
use hashbrown::hash_map::{Entry, HashMap};
use s2n_quic_core::{connection, endpoint, random, stateless_reset, time::Timestamp};
use siphasher::sip::SipHasher13;
use std::sync::{Arc, Mutex};

// Since the input to the hash function (stateless reset token) come from the peer, we need to
// ensure that maliciously crafted values do not result in poor bucketing and thus degraded
// performance. To accomplish this, we generate random keys when the StatelessResetMap is
// initialized, and use those keys each time the hash function is called. The resulting computed
// hash values are thus not completely based on external input making it difficult to craft values
// that would result in poor bucketing. This is the same process `std::collections::HashMap` performs
// to protect against such attacks. We implement this explicitly to ensure this map continues to
// provide this protection even if future versions of `std::collections::HashMap` do not and to
// make the hash algorithm used explicit.
#[derive(Debug)]
pub struct HashState {
    k0: u64,
    k1: u64,
}

impl HashState {
    /// Generates hash state by using the given random generator to produce random keys.
    fn new(random_generator: &mut dyn random::Generator) -> HashState {
        let mut k0 = [0u8; core::mem::size_of::<u64>()];
        let mut k1 = [0u8; core::mem::size_of::<u64>()];

        random_generator.private_random_fill(&mut k0);
        random_generator.private_random_fill(&mut k1);

        Self {
            k0: u64::from_be_bytes(k0),
            k1: u64::from_be_bytes(k1),
        }
    }
}

impl BuildHasher for HashState {
    type Hasher = SipHasher13;

    /// Builds a hasher using the hash state keys
    fn build_hasher(&self) -> Self::Hasher {
        SipHasher13::new_with_keys(self.k0, self.k1)
    }
}

#[derive(Debug)]
pub(crate) struct StatelessResetMap {
    /// Maps from a hash of peer stateless reset token to internal connection IDs
    map: HashMap<stateless_reset::Token, InternalConnectionId, HashState>,
}

impl StatelessResetMap {
    /// Constructs a new `StatelessResetMap`
    fn new(hash_state: HashState) -> Self {
        Self {
            map: HashMap::with_hasher(hash_state),
        }
    }

    /// Inserts the given stateless reset token and the given
    /// internal connection ID into the stateless reset map.
    pub(crate) fn insert(
        &mut self,
        token: stateless_reset::Token,
        internal_id: InternalConnectionId,
    ) {
        self.map.insert(token, internal_id);
    }

    /// Removes the mapping for the given key, returning the
    /// `InternalConnection` if it was in the map.
    pub(crate) fn remove(
        &mut self,
        token: &stateless_reset::Token,
    ) -> Option<InternalConnectionId> {
        self.map.remove(token)
    }
}

#[derive(Debug)]
pub(crate) struct LocalIdMap {
    /// Maps from external to internal connection IDs
    map: HashMap<connection::LocalId, InternalConnectionId, HashState>,
}

impl LocalIdMap {
    /// Constructs a new `LocalIdMap`
    fn new(hash_state: HashState) -> Self {
        Self {
            map: HashMap::with_hasher(hash_state),
        }
    }

    /// Gets the `InternalConnectionId` (if any) associated with the given local id
    pub(crate) fn get(&self, local_id: &connection::LocalId) -> Option<InternalConnectionId> {
        self.map.get(local_id).copied()
    }

    /// Inserts the given `LocalId` into the map if it is not already in the map,
    /// otherwise returns an Err
    pub(crate) fn try_insert(
        &mut self,
        local_id: &connection::LocalId,
        internal_id: InternalConnectionId,
    ) -> Result<(), ()> {
        let entry = self.map.entry(*local_id);
        match entry {
            Entry::Occupied(_) => Err(()),
            Entry::Vacant(entry) => {
                entry.insert(internal_id);
                Ok(())
            }
        }
    }

    /// Removes the given `LocalId` from the map
    pub(crate) fn remove(
        &mut self,
        local_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        self.map.remove(local_id)
    }
}

/// Bidirectional map for mapping from initial ID to internal connection ID and vice-versa
#[derive(Debug)]
pub(crate) struct InitialIdMap {
    /// Maps from initial id to internal connection ID
    initial_to_internal_id_map: HashMap<connection::InitialId, InternalConnectionId, HashState>,
    /// Maps from internal connection ID to initial ID
    internal_to_initial_id_map: HashMap<InternalConnectionId, connection::InitialId, HashState>,
}

impl InitialIdMap {
    /// Constructs a new `InitialIdMap`
    fn new(
        initial_to_internal_hash_state: HashState,
        internal_to_initial_hash_state: HashState,
    ) -> Self {
        Self {
            initial_to_internal_id_map: HashMap::with_hasher(initial_to_internal_hash_state),
            internal_to_initial_id_map: HashMap::with_hasher(internal_to_initial_hash_state),
        }
    }

    /// Gets the `InternalConnectionId` (if any) associated with the given initial id
    fn get(&self, initial_id: &connection::InitialId) -> Option<InternalConnectionId> {
        self.initial_to_internal_id_map.get(initial_id).copied()
    }

    /// Inserts the given `InitialId` into the map if it is not already in the map,
    /// otherwise returns an Err
    fn try_insert(
        &mut self,
        initial_id: connection::InitialId,
        internal_id: InternalConnectionId,
    ) -> Result<(), ()> {
        let initial_to_internal_id_entry = self.initial_to_internal_id_map.entry(initial_id);
        let internal_to_initial_id_entry = self.internal_to_initial_id_map.entry(internal_id);

        match (initial_to_internal_id_entry, internal_to_initial_id_entry) {
            (Entry::Occupied(_), _) | (_, Entry::Occupied(_)) => Err(()),
            (Entry::Vacant(initial_entry), Entry::Vacant(internal_entry)) => {
                initial_entry.insert(internal_id);
                internal_entry.insert(initial_id);
                Ok(())
            }
        }
    }

    /// Removes the `InitialId` associated with the given `InternalConnectionId` from the map
    pub(crate) fn remove(
        &mut self,
        internal_id: &InternalConnectionId,
    ) -> Option<connection::InitialId> {
        let initial_id = self.internal_to_initial_id_map.remove(internal_id)?;
        self.initial_to_internal_id_map.remove(&initial_id);
        Some(initial_id)
    }
}

/// Bidirectional map for mapping from initial ID to internal connection ID and vice-versa
#[derive(Debug)]
pub(crate) struct OpenRequestMap {
    /// Maps from initial id to internal connection ID
    // No need for custom hashing since keys are locally controlled, not by remote.
    open_request_map: HashMap<crate::endpoint::connect::Connect, InternalConnectionId>,
}

impl OpenRequestMap {
    /// Constructs a new `InitialIdMap`
    fn new() -> Self {
        Self {
            open_request_map: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionIdMapperState {
    /// Maps from external to internal connection IDs
    pub(crate) local_id_map: LocalIdMap,
    /// Maps from a hash of peer stateless reset token to internal connection IDs
    pub(crate) stateless_reset_map: StatelessResetMap,
    /// Maps from initial id to internal connection IDs
    pub(crate) initial_id_map: InitialIdMap,
    /// Maps from connection open request to internal connection IDs
    /// This is used for looking up a connection handle if one is already open,
    /// rather than opening a new one each time.
    pub(crate) open_request_map: OpenRequestMap,
}

impl ConnectionIdMapperState {
    fn new(random_generator: &mut dyn random::Generator) -> Self {
        Self {
            local_id_map: LocalIdMap::new(HashState::new(random_generator)),
            stateless_reset_map: StatelessResetMap::new(HashState::new(random_generator)),
            initial_id_map: InitialIdMap::new(
                HashState::new(random_generator),
                HashState::new(random_generator),
            ),
            open_request_map: OpenRequestMap::new(),
        }
    }
}

/// Maps from external connection IDs to internal connection IDs
pub struct ConnectionIdMapper {
    /// The shared state between mapper and registration
    state: Arc<Mutex<ConnectionIdMapperState>>,
    /// The endpoint type for the endpoint using this mapper
    endpoint_type: endpoint::Type,
}

impl ConnectionIdMapper {
    /// Creates a new `ConnectionIdMapper`
    pub fn new(
        random_generator: &mut dyn random::Generator,
        endpoint_type: endpoint::Type,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConnectionIdMapperState::new(random_generator))),
            endpoint_type,
        }
    }

    /// Looks up the internal Connection ID which is associated with an external
    /// connection ID.
    pub fn lookup_internal_connection_id(
        &self,
        connection_id: &connection::LocalId,
    ) -> Option<(InternalConnectionId, connection::id::Classification)> {
        let guard = self
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned");
        guard
            .local_id_map
            .get(connection_id)
            .map(|id| (id, connection::id::Classification::Local))
            .or_else(|| {
                if self.endpoint_type.is_server() {
                    // The ID wasn't in the local ID map, so we'll check the initial ID
                    // map in case this ID was from a duplicate initial packet
                    connection::InitialId::try_from(*connection_id)
                        .ok()
                        .and_then(|initial_id| guard.initial_id_map.get(&initial_id))
                        .map(|id| (id, connection::id::Classification::Initial))
                } else {
                    None
                }
            })
    }

    /// Inserts the given `InitialId` into the map if it is not already in the map,
    /// otherwise returns an Err
    pub fn try_insert_initial_id(
        &mut self,
        initial_id: connection::InitialId,
        internal_id: InternalConnectionId,
    ) -> Result<(), ()> {
        debug_assert!(self.endpoint_type.is_server());
        let mut guard = self
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned");
        guard.initial_id_map.try_insert(initial_id, internal_id)
    }

    /// Looks up the internal Connection ID which is associated with a stateless
    /// reset token and removes it from the map if it was found.
    #[must_use]
    pub fn remove_internal_connection_id_by_stateless_reset_token(
        &mut self,
        peer_stateless_reset_token: &stateless_reset::Token,
    ) -> Option<InternalConnectionId> {
        let mut guard = self
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned");
        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
        //# When comparing a datagram to stateless reset token values, endpoints
        //# MUST perform the comparison without leaking information about the
        //# value of the token.
        // The given value is hashed using SipHash13 which is a secure PRF and
        // randomly keyed for each instance. This means that looking up on and comparing
        // against this hash gives no useful timing information to an observer. (Namely,
        // it will leak if there is a stateless reset token with the same hashed value,
        // but no information about the reset token itself.) Actual equality checks for
        // stateless reset tokens are implemented in stateless_reset::Token in
        // a constant-time manner.
        guard.stateless_reset_map.remove(peer_stateless_reset_token)
    }

    /// Removes the initial id mapping associated with the given internal ID
    pub fn remove_initial_id(
        &mut self,
        internal_id: &InternalConnectionId,
    ) -> Option<connection::InitialId> {
        debug_assert!(self.endpoint_type.is_server());
        let mut guard = self
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned");
        guard.initial_id_map.remove(internal_id)
    }

    /// Creates a `LocalIdRegistry` for a new internal connection ID, which allows that
    /// connection to modify the mappings of it's Connection ID aliases. The provided
    /// `initial_connection_id` will be registered in the returned registry.
    pub fn create_local_id_registry(
        &mut self,
        internal_id: InternalConnectionId,
        initial_connection_id: &connection::LocalId,
        initial_connection_id_expiration_time: Option<Timestamp>,
        local_stateless_reset_token: stateless_reset::Token,
        rotate_handshake_connection_id: bool,
    ) -> LocalIdRegistry {
        LocalIdRegistry::new(
            internal_id,
            self.state.clone(),
            initial_connection_id,
            initial_connection_id_expiration_time,
            local_stateless_reset_token,
            rotate_handshake_connection_id,
        )
    }

    /// Creates a Server `PeerIdRegistry` for a new InternalConnectionId.
    ///
    /// The registry allows a connection to modify the mappings of it's
    /// Connection ID aliases. The provided `initial_connection_id`
    /// will be registered in the returned registry.
    pub fn create_server_peer_id_registry(
        &mut self,
        internal_id: InternalConnectionId,
        initial_connection_id: connection::PeerId,
        rotate_handshake_connection_id: bool,
    ) -> PeerIdRegistry {
        let mut registry = PeerIdRegistry::new(
            internal_id,
            self.state.clone(),
            rotate_handshake_connection_id,
        );

        registry.register_initial_connection_id(initial_connection_id);
        registry
    }

    /// Creates a Client `PeerIdRegistry` for a new InternalConnectionId.
    ///
    /// Similar to [`Self::create_server_peer_id_registry`] but does not register
    /// an initial_connection_id since one it is only available after the first
    /// Server response.
    pub fn create_client_peer_id_registry(
        &mut self,
        internal_id: InternalConnectionId,
        rotate_handshake_connection_id: bool,
    ) -> PeerIdRegistry {
        PeerIdRegistry::new(
            internal_id,
            self.state.clone(),
            rotate_handshake_connection_id,
        )
    }

    /// Returns the internal connection ID corresponding to the connect request, if there is a
    /// pending or already open connection for that ID.
    ///
    /// If no such connection exists, associates the connect request with the provided internal ID,
    /// which is returned in future requests.
    pub(crate) fn lazy_open(
        &self,
        new_connection_internal_id: InternalConnectionId,
        connect: crate::endpoint::connect::Connect,
    ) -> Result<InternalConnectionId, OpenRegistry> {
        let mut guard = self.state.lock().unwrap();
        match guard.open_request_map.open_request_map.entry(connect) {
            Entry::Occupied(e) => Ok(*e.get()),
            Entry::Vacant(e) => {
                let connect = e.key().clone();
                e.insert(new_connection_internal_id);
                Err(OpenRegistry {
                    state: self.state.clone(),
                    connect,
                })
            }
        }
    }
}

#[derive(Debug)]
pub struct OpenRegistry {
    /// The shared state between mapper and registration
    state: Arc<Mutex<ConnectionIdMapperState>>,
    connect: crate::endpoint::connect::Connect,
}

impl Drop for OpenRegistry {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            // Stop tracking this open connection.
            guard
                .open_request_map
                .open_request_map
                .remove(&self.connect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{peer_id_registry::testing::id, InternalConnectionIdGenerator};
    use s2n_quic_core::stateless_reset::token::testing::*;

    #[test]
    fn remove_internal_connection_id_by_stateless_reset_token_test() {
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();
        let peer_id = id(b"id01");

        let mut registry = mapper.create_client_peer_id_registry(internal_id, true);
        registry.register_initial_connection_id(peer_id);
        registry.register_initial_stateless_reset_token(TEST_TOKEN_1);

        assert_eq!(
            Some(internal_id),
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
        );
        assert_eq!(
            None,
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
        );
        assert_eq!(
            None,
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_2)
        );

        let mut registry = mapper.create_client_peer_id_registry(internal_id, true);
        registry.register_initial_connection_id(peer_id);
        registry.register_initial_stateless_reset_token(TEST_TOKEN_3);

        mapper
            .state
            .lock()
            .expect("should succeed unless the lock is poisoned")
            .stateless_reset_map
            .remove(&TEST_TOKEN_3);

        assert_eq!(
            None,
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_3)
        );
    }

    #[test]
    fn initial_id_map() {
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();
        let local_id = connection::LocalId::try_from_bytes(b"id000001").unwrap();
        let initial_id = connection::InitialId::try_from(local_id).unwrap();

        assert_eq!(None, mapper.lookup_internal_connection_id(&local_id));

        assert!(mapper
            .try_insert_initial_id(initial_id, internal_id)
            .is_ok());
        assert!(mapper
            .try_insert_initial_id(initial_id, internal_id)
            .is_err());

        assert_eq!(
            Some((internal_id, connection::id::Classification::Initial,)),
            mapper.lookup_internal_connection_id(&local_id)
        );

        assert_eq!(Some(initial_id), mapper.remove_initial_id(&internal_id));

        assert_eq!(None, mapper.lookup_internal_connection_id(&local_id));
    }

    #[test]
    #[should_panic]
    fn initial_id_map_client_insert() {
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Client);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();
        let local_id = connection::LocalId::try_from_bytes(b"id000001").unwrap();
        let initial_id = connection::InitialId::try_from(local_id).unwrap();

        assert_eq!(None, mapper.lookup_internal_connection_id(&local_id));

        let _ = mapper.try_insert_initial_id(initial_id, internal_id);
    }

    #[test]
    #[should_panic]
    fn initial_id_map_client_remove() {
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Client);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();

        mapper.remove_initial_id(&internal_id);
    }

    #[test]
    fn initial_id_map_client_lookup() {
        let mut random_generator = random::testing::Generator(123);
        let mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Client);
        let local_id = connection::LocalId::try_from_bytes(b"id000001").unwrap();

        assert_eq!(None, mapper.lookup_internal_connection_id(&local_id));
    }
}
