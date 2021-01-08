//! Maps from external connection IDs to internal connection IDs

use alloc::rc::Rc;
use core::cell::RefCell;
use std::collections::hash_map::{Entry, HashMap};

use s2n_quic_core::{connection, stateless_reset};

use crate::connection::{local_id_registry::LocalIdRegistry, InternalConnectionId, PeerIdRegistry};
use s2n_quic_core::inet::SocketAddress;

#[derive(Debug)]
pub(crate) struct ConnectionIdMapperState {
    /// Maps from external to internal connection IDs
    connection_map: HashMap<connection::LocalId, InternalConnectionId>,
    /// Maps from a tuple of peer stateless reset token and remote address to internal connection IDs
    stateless_reset_token_map:
        HashMap<(stateless_reset::Token, SocketAddress), InternalConnectionId>,
}

impl ConnectionIdMapperState {
    fn new() -> Self {
        Self {
            connection_map: HashMap::new(),
            stateless_reset_token_map: HashMap::new(),
        }
    }

    pub(crate) fn try_insert_local_id(
        &mut self,
        external_id: &connection::LocalId,
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

    pub(crate) fn insert_stateless_reset_token(
        &mut self,
        token: stateless_reset::Token,
        remote_address: SocketAddress,
        internal_id: InternalConnectionId,
    ) {
        self.stateless_reset_token_map
            .insert((token, remote_address), internal_id);
    }

    pub(crate) fn remove_local_id(
        &mut self,
        external_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        self.connection_map.remove(external_id)
    }

    pub(crate) fn remove_stateless_reset_token(
        &mut self,
        token: stateless_reset::Token,
        remote_address: SocketAddress,
    ) -> Option<InternalConnectionId> {
        self.stateless_reset_token_map
            .remove(&(token, remote_address))
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
        connection_id: &connection::LocalId,
    ) -> Option<InternalConnectionId> {
        let guard = self.state.borrow();
        guard.connection_map.get(connection_id).map(Clone::clone)
    }

    /// Looks up the internal Connection ID which is associated with a stateless
    /// reset token and remote address.
    #[allow(dead_code)] //TODO: Remove when used
    pub fn lookup_internal_connection_id_by_stateless_reset_token(
        &self,
        peer_stateless_reset_token: stateless_reset::Token,
        remote_address: SocketAddress,
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
            .stateless_reset_token_map
            .get(&(peer_stateless_reset_token, remote_address))
            .map(Clone::clone)
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
