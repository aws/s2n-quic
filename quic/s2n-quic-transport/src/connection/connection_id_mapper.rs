//! Maps from external connection IDs to internal connection IDs

use crate::connection::{local_id_registry::LocalIdRegistry, InternalConnectionId, PeerIdRegistry};
use alloc::rc::Rc;
use core::{cell::RefCell, hash::BuildHasher};
use hashbrown::hash_map::{Entry, HashMap};
use s2n_quic_core::{connection, random, stateless_reset};
use siphasher::sip::SipHasher13;

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
    fn new<R: random::Generator>(random_generator: &mut R) -> HashState {
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
        self.map.get(&local_id).copied()
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

#[derive(Debug)]
pub(crate) struct ConnectionIdMapperState {
    /// Maps from external to internal connection IDs
    pub(crate) local_id_map: LocalIdMap,
    /// Maps from a hash of peer stateless reset token to internal connection IDs
    pub(crate) stateless_reset_map: StatelessResetMap,
}

impl ConnectionIdMapperState {
    fn new<R: random::Generator>(random_generator: &mut R) -> Self {
        Self {
            local_id_map: LocalIdMap::new(HashState::new(random_generator)),
            stateless_reset_map: StatelessResetMap::new(HashState::new(random_generator)),
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
    pub fn new<R: random::Generator>(random_generator: &mut R) -> Self {
        Self {
            state: Rc::new(RefCell::new(ConnectionIdMapperState::new(random_generator))),
        }
    }

    /// Looks up the internal Connection ID which is associated with an external
    /// connection ID.
    pub fn lookup_internal_connection_id(
        &self,
        connection_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        let guard = self.state.borrow();
        guard.local_id_map.get(connection_id)
    }

    /// Looks up the internal Connection ID which is associated with a stateless
    /// reset token and removes it from the map if it was found.
    #[must_use]
    pub fn remove_internal_connection_id_by_stateless_reset_token(
        &mut self,
        peer_stateless_reset_token: &stateless_reset::Token,
    ) -> Option<InternalConnectionId> {
        let mut guard = self.state.borrow_mut();
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
        //# When comparing a datagram to Stateless Reset Token values, endpoints
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

    /// Creates a `LocalIdRegistry` for a new internal connection ID, which allows that
    /// connection to modify the mappings of it's Connection ID aliases. The provided
    /// `initial_connection_id` will be registered in the returned registry.
    pub fn create_local_id_registry(
        &mut self,
        internal_id: InternalConnectionId,
        initial_connection_id: &connection::LocalId,
        local_stateless_reset_token: stateless_reset::Token,
    ) -> LocalIdRegistry {
        LocalIdRegistry::new(
            internal_id,
            self.state.clone(),
            initial_connection_id,
            local_stateless_reset_token,
        )
    }

    /// Creates a `PeerIdRegistry` for a new internal connection ID, which allows that
    /// connection to modify the mappings of it's Connection ID aliases. The provided
    /// `initial_connection_id` will be registered in the returned registry.
    pub fn create_peer_id_registry(
        &mut self,
        internal_id: InternalConnectionId,
        initial_connection_id: connection::PeerId,
        peer_stateless_reset_token: Option<stateless_reset::Token>,
    ) -> PeerIdRegistry {
        PeerIdRegistry::new(
            internal_id,
            self.state.clone(),
            initial_connection_id,
            peer_stateless_reset_token,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{peer_id_registry::tests::id, InternalConnectionIdGenerator};
    use s2n_quic_core::stateless_reset::token::testing::*;

    #[test]
    fn remove_internal_connection_id_by_stateless_reset_token_test() {
        let mut random_generator = random::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut random_generator);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();
        let peer_id = id(b"id01");

        let _registry = mapper.create_peer_id_registry(internal_id, peer_id, Some(TEST_TOKEN_1));

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

        let _registry = mapper.create_peer_id_registry(internal_id, peer_id, Some(TEST_TOKEN_3));

        mapper
            .state
            .borrow_mut()
            .stateless_reset_map
            .remove(&TEST_TOKEN_3);

        assert_eq!(
            None,
            mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_3)
        );
    }
}
