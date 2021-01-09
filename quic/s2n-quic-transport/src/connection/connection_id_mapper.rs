//! Maps from external connection IDs to internal connection IDs

use alloc::rc::Rc;
use core::cell::RefCell;
use hash_hasher::HashBuildHasher;
use hashbrown::hash_map::{Entry, HashMap};
use siphasher::sip::SipHasher13;

use s2n_quic_core::{connection, stateless_reset};

use crate::connection::{local_id_registry::LocalIdRegistry, InternalConnectionId, PeerIdRegistry};
use core::{
    hash::{Hash, Hasher},
    num::NonZeroU64,
};
use s2n_quic_core::inet::SocketAddress;

// Since the inputs to the hash function (stateless reset token and remote address) come from the
// peer, we need to ensure that maliciously crafted values do not result in poor bucketing and thus
// degraded performance. To accomplish this, we generate random keys when the StatelessResetMap is
// initialized, and use those keys each time the hash function is called. The resulting computed
// hash values are thus not completely based on external input making it difficult to craft values
// that would result in poor bucketing. This is the same process `std::collections::HashMap` performs
// to protect against such attacks.
#[derive(Debug)]
pub struct HashState {
    k0: u64,
    k1: u64,
}

impl HashState {
    /// Generates hash state by using the given unpredictable bits generator to produce
    /// random keys.
    fn new<U: stateless_reset::UnpredictableBits>(
        unpredictable_bits_generator: &mut U,
    ) -> HashState {
        let mut k0 = [0u8; core::mem::size_of::<u64>()];
        let mut k1 = [0u8; core::mem::size_of::<u64>()];

        unpredictable_bits_generator.fill(&mut k0);
        unpredictable_bits_generator.fill(&mut k1);

        Self {
            k0: u64::from_be_bytes(k0),
            k1: u64::from_be_bytes(k1),
        }
    }

    /// Builds a hasher using the hash state keys
    fn build_hasher(&self) -> SipHasher13 {
        SipHasher13::new_with_keys(self.k0, self.k1)
    }
}

/// A handle to an entry in the `StatelessResetMap`
#[derive(Debug, Clone, Copy, Eq, PartialOrd, PartialEq, Ord)]
pub struct StatelessResetHandle(NonZeroU64);

#[derive(Debug)]
pub(crate) struct StatelessResetMap {
    /// Maps from a hash of peer stateless reset token and remote address to internal connection IDs
    map: HashMap<NonZeroU64, InternalConnectionId, HashBuildHasher>,
    /// Hash state for use when initializing the hash function
    hash_state: HashState,
}

impl StatelessResetMap {
    /// A prechecked value of 1 used if the hash happens to be zero
    const ONE: NonZeroU64 = unsafe { NonZeroU64::new_unchecked(1) };

    /// Constructs a new `StatelessResetMap`
    fn new(hash_state: HashState) -> Self {
        Self {
            // The `HashBuildHasher`, which doesn't perform additional hashing on the key, is used
            // since the key is already a hash of Stateless Reset Token and Remote Address
            map: HashMap::with_hasher(HashBuildHasher::default()),
            hash_state,
        }
    }

    /// Gets the `InternalConnectionId` (if any) associated with the given stateless reset token
    /// and remote address
    fn get(
        &self,
        token: &stateless_reset::Token,
        remote_address: &SocketAddress,
    ) -> Option<InternalConnectionId> {
        let handle = self.handle(token, remote_address);

        self.map.get(&handle.0).map(Clone::clone)
    }

    /// Inserts a hash of the given stateless reset token and remote address, and the given
    /// internal connection ID into the stateless reset map.
    fn insert(&mut self, handle: StatelessResetHandle, internal_id: InternalConnectionId) {
        self.map.insert(handle.0, internal_id);
    }

    /// Removes the mapping for the given key
    fn remove(&mut self, handle: StatelessResetHandle) -> Option<InternalConnectionId> {
        self.map.remove(&handle.0)
    }

    /// Creates a handle to an entry in the stateless reset map by hashing the given stateless
    /// reset token and remote address
    fn handle(
        &self,
        token: &stateless_reset::Token,
        remote_address: &SocketAddress,
    ) -> StatelessResetHandle {
        let mut hasher = self.hash_state.build_hasher();
        token.hash(&mut hasher);
        remote_address.hash(&mut hasher);
        StatelessResetHandle {
            0: NonZeroU64::new(hasher.finish()).unwrap_or(Self::ONE),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionIdMapperState {
    /// Maps from external to internal connection IDs
    local_id_map: HashMap<connection::LocalId, InternalConnectionId>,
    /// Maps from a hash of peer stateless reset token and remote address to internal connection IDs
    stateless_reset_map: StatelessResetMap,
}

impl ConnectionIdMapperState {
    fn new(hash_state: HashState) -> Self {
        Self {
            local_id_map: HashMap::new(),
            stateless_reset_map: StatelessResetMap::new(hash_state),
        }
    }

    pub(crate) fn try_insert_local_id(
        &mut self,
        external_id: &connection::LocalId,
        internal_id: InternalConnectionId,
    ) -> Result<(), ()> {
        let entry = self.local_id_map.entry(*external_id);
        match entry {
            Entry::Occupied(_) => Err(()),
            Entry::Vacant(entry) => {
                entry.insert(internal_id);
                Ok(())
            }
        }
    }

    /// Inserts a hash of the given stateless reset token and remote address, and the given
    /// internal connection ID into the stateless reset map.
    pub(crate) fn insert_stateless_reset_token(
        &mut self,
        handle: StatelessResetHandle,
        internal_id: InternalConnectionId,
    ) {
        self.stateless_reset_map.insert(handle, internal_id);
    }

    pub(crate) fn remove_local_id(
        &mut self,
        external_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        self.local_id_map.remove(external_id)
    }

    /// Removes the stateless reset token associated with the given `StatelessResetHandle` from
    /// the stateless reset map.
    pub(crate) fn remove_stateless_reset_token(
        &mut self,
        handle: StatelessResetHandle,
    ) -> Option<InternalConnectionId> {
        self.stateless_reset_map.remove(handle)
    }

    /// Creates a handle to an entry in the stateless reset map by hashing the given stateless
    /// reset token and remote address
    pub(crate) fn stateless_reset_handle(
        &self,
        token: &stateless_reset::Token,
        remote_address: &SocketAddress,
    ) -> StatelessResetHandle {
        self.stateless_reset_map.handle(token, remote_address)
    }
}

/// Maps from external connection IDs to internal connection IDs
pub struct ConnectionIdMapper {
    /// The shared state between mapper and registration
    state: Rc<RefCell<ConnectionIdMapperState>>,
}

impl ConnectionIdMapper {
    /// Creates a new `ConnectionIdMapper`
    pub fn new<U: stateless_reset::UnpredictableBits>(
        unpredictable_bits_generator: &mut U,
    ) -> Self {
        let random_state = HashState::new(unpredictable_bits_generator);

        Self {
            state: Rc::new(RefCell::new(ConnectionIdMapperState::new(random_state))),
        }
    }

    /// Looks up the internal Connection ID which is associated with an external
    /// connection ID.
    pub fn lookup_internal_connection_id(
        &self,
        connection_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        let guard = self.state.borrow();
        guard.local_id_map.get(connection_id).map(Clone::clone)
    }

    /// Looks up the internal Connection ID which is associated with a stateless
    /// reset token and remote address.
    #[allow(dead_code)] //TODO: Remove when used
    pub fn lookup_internal_connection_id_by_stateless_reset_token(
        &self,
        peer_stateless_reset_token: &stateless_reset::Token,
        remote_address: &SocketAddress,
    ) -> Option<InternalConnectionId> {
        let guard = self.state.borrow();
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
        //# When comparing a datagram to Stateless Reset Token values, endpoints
        //# MUST perform the comparison without leaking information about the
        //# value of the token.
        // The given token is compared to the known Stateless Reset Token values by
        // looking it up in a HashMap, which will perform a hashing function on the
        // key regardless of its value. Since the token is a fixed length, this will
        // be constant time.
        guard
            .stateless_reset_map
            .get(peer_stateless_reset_token, remote_address)
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
        remote_address: SocketAddress,
    ) -> PeerIdRegistry {
        PeerIdRegistry::new(
            internal_id,
            self.state.clone(),
            initial_connection_id,
            peer_stateless_reset_token,
            remote_address,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{peer_id_registry::tests::id, InternalConnectionIdGenerator};
    use s2n_quic_core::stateless_reset::token::testing::*;

    #[test]
    fn lookup_internal_connection_id_by_stateless_reset_token_test() {
        let mut unpredictable_bits_generator = stateless_reset::testing::Generator(123);
        let mut mapper = ConnectionIdMapper::new(&mut unpredictable_bits_generator);
        let internal_id = InternalConnectionIdGenerator::new().generate_id();
        let peer_id = id(b"id01");
        let remote_address = SocketAddress::default();

        let _registry = mapper.create_peer_id_registry(
            internal_id,
            peer_id,
            Some(TEST_TOKEN_1),
            remote_address,
        );

        assert_eq!(
            Some(internal_id),
            mapper.lookup_internal_connection_id_by_stateless_reset_token(
                &TEST_TOKEN_1,
                &remote_address
            )
        );
        assert_eq!(
            None,
            mapper.lookup_internal_connection_id_by_stateless_reset_token(
                &TEST_TOKEN_2,
                &remote_address
            )
        );
        assert_eq!(
            None,
            mapper.lookup_internal_connection_id_by_stateless_reset_token(
                &TEST_TOKEN_1,
                &SocketAddress::IPv6(Default::default())
            )
        );

        let handle = mapper
            .state
            .borrow()
            .stateless_reset_handle(&TEST_TOKEN_1, &remote_address);
        mapper
            .state
            .borrow_mut()
            .remove_stateless_reset_token(handle);

        assert_eq!(
            None,
            mapper.lookup_internal_connection_id_by_stateless_reset_token(
                &TEST_TOKEN_1,
                &remote_address
            )
        );
    }

    #[test]
    fn stateless_reset_handle_test() {
        let mut unpredictable_bits_generator = stateless_reset::testing::Generator(123);
        let mapper = ConnectionIdMapper::new(&mut unpredictable_bits_generator);
        let mapper = mapper.state.borrow();
        let remote_address = SocketAddress::default();

        let handle_1 = mapper.stateless_reset_handle(&TEST_TOKEN_1, &remote_address);
        let handle_2 = mapper.stateless_reset_handle(&TEST_TOKEN_1, &remote_address);

        assert_eq!(handle_1, handle_2);

        let handle_3 = mapper.stateless_reset_handle(&TEST_TOKEN_2, &remote_address);
        let handle_4 =
            mapper.stateless_reset_handle(&TEST_TOKEN_1, &SocketAddress::IPv6(Default::default()));

        assert_ne!(handle_1, handle_3);
        assert_ne!(handle_1, handle_4);

        let mapper_2 = ConnectionIdMapper::new(&mut unpredictable_bits_generator);
        let mapper_2 = mapper_2.state.borrow();

        let handle_5 = mapper_2.stateless_reset_handle(&TEST_TOKEN_1, &remote_address);
        let handle_6 = mapper_2.stateless_reset_handle(&TEST_TOKEN_1, &remote_address);

        assert_eq!(handle_5, handle_6);
        assert_ne!(handle_1, handle_5);
    }
}
