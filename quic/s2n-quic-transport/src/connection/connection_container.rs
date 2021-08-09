// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `ConnectionContainer` is a container for all Connections. It manages the permanent
//! map of all active Connections, as well as a variety of dynamic Connection lists.

use crate::{
    connection::{
        self, Connection, ConnectionInterests, InternalConnectionId, SharedConnectionState,
        SynchronizedSharedConnectionState, Trait as ConnectionTrait,
    },
    unbounded_channel,
};
use alloc::sync::Arc;
use core::{
    cell::{Cell, RefCell},
    ops::Deref,
};
use intrusive_collections::{
    intrusive_adapter, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use s2n_quic_core::{recovery::K_GRANULARITY, time::Timestamp};

// Intrusive list adapter for managing the list of `done` connections
intrusive_adapter!(DoneConnectionsAdapter<C> = Arc<ConnectionNode<C>>: ConnectionNode<C> {
    done_connections_link: LinkedListLink
} where C: connection::Trait);

// Intrusive list adapter for managing the list of
// `waiting_for_transmission` connections
intrusive_adapter!(WaitingForTransmissionAdapter<C> = Arc<ConnectionNode<C>>: ConnectionNode<C> {
    waiting_for_transmission_link: LinkedListLink
} where C: connection::Trait);

// Intrusive list adapter for managing the list of
// `waiting_for_connection_id` connections
intrusive_adapter!(WaitingForConnectionIdAdapter<C> = Arc<ConnectionNode<C>>: ConnectionNode<C> {
    waiting_for_connection_id_link: LinkedListLink
} where C: ConnectionTrait);

// Intrusive red black tree adapter for managing a list of `waiting_for_timeout` connections
intrusive_adapter!(WaitingForTimeoutAdapter<C> = Arc<ConnectionNode<C>>: ConnectionNode<C> {
    waiting_for_timeout_link: RBTreeLink
} where C: ConnectionTrait);

// Intrusive red black tree adapter for managing all connections in a tree for
// lookup by Connection ID
intrusive_adapter!(ConnectionTreeAdapter<C> = Arc<ConnectionNode<C>>: ConnectionNode<C> {
    tree_link: RBTreeLink
} where C: ConnectionTrait);

/// A wrapper around a `Connection` implementation which allows to insert the
/// it in multiple intrusive collections. The collections into which the `Connection`
/// gets inserted are referenced inside this `ConnectionNode`.
struct ConnectionNode<C: ConnectionTrait> {
    /// This contains the actual implementation of the `Connection`
    inner: RefCell<C>,
    /// The shared state associated with the connection
    shared_state: RefCell<State<C>>,
    /// Allows the Connection to be part of the `connection_map` collection
    tree_link: RBTreeLink,
    /// Allows the Connection to be part of the `done_connections` collection
    done_connections_link: LinkedListLink,
    /// Allows the Connection to be part of the `waiting_for_transmission` collection
    waiting_for_transmission_link: LinkedListLink,
    /// Allows the Connection to be part of the `waiting_for_connection_id` collection
    waiting_for_connection_id_link: LinkedListLink,
    /// Allows the Connection to be part of the `waiting_for_timeout` collection
    waiting_for_timeout_link: RBTreeLink,
    /// The cached time at which the connection will timeout next
    timeout: Cell<Option<Timestamp>>,
}

enum State<C: ConnectionTrait> {
    Connected(Arc<SynchronizedSharedConnectionState<C::Config>>),
    Closing,
}

impl<C: ConnectionTrait> State<C> {
    fn try_lock(&self) -> Option<std::sync::MutexGuard<'_, SharedConnectionState<C::Config>>> {
        match self {
            Self::Connected(state) => Some(state.lock()),
            Self::Closing => None,
        }
    }
}

impl<C: ConnectionTrait> ConnectionNode<C> {
    /// Creates a new `ConnectionNode` which wraps the given Connection implementation of type `S`
    pub fn new(
        connection_impl: C,
        shared_state: Arc<SynchronizedSharedConnectionState<C::Config>>,
    ) -> ConnectionNode<C> {
        ConnectionNode {
            inner: RefCell::new(connection_impl),
            shared_state: RefCell::new(State::Connected(shared_state)),
            tree_link: RBTreeLink::new(),
            done_connections_link: LinkedListLink::new(),
            waiting_for_transmission_link: LinkedListLink::new(),
            waiting_for_connection_id_link: LinkedListLink::new(),
            waiting_for_timeout_link: RBTreeLink::new(),
            timeout: Cell::new(None),
        }
    }

    /// Obtains a `Arc<ConnectionNode>` from a `&ConnectionNode`.
    ///
    /// This method is only safe to be called if the `ConnectionNode` is known to be
    /// stored inside a `Arc`.
    unsafe fn arc_from_ref(&self) -> Arc<Self> {
        // In order to be able to to get a `Arc` we construct a temporary `Arc`
        // from it using the `Arc::from_raw` API and clone the `Arc`.
        // The temporary `Arc` must be released without calling `drop`,
        // because this would decrement and thereby invalidate the refcount
        // (which wasn't changed by calling `Arc::from_raw`).
        let temp_node_ptr: core::mem::ManuallyDrop<Arc<ConnectionNode<C>>> =
            core::mem::ManuallyDrop::new(Arc::<ConnectionNode<C>>::from_raw(
                self as *const ConnectionNode<C>,
            ));

        temp_node_ptr.deref().clone()
    }
}

impl<'a, C: connection::Trait> KeyAdapter<'a> for WaitingForTimeoutAdapter<C> {
    type Key = Timestamp;

    fn get_key(&self, node: &'a ConnectionNode<C>) -> Timestamp {
        if let Some(timeout) = node.timeout.get() {
            timeout
        } else if cfg!(debug_assertions) {
            panic!("node was queried for timeout but none was set")
        } else {
            unsafe {
                // Safety: this will simply move the connection to the beginning of the queue
                // to ensure the timeout value is properly updated.
                //
                // Assuming everything is tested properly, this should never be reached
                Timestamp::from_duration(core::time::Duration::from_secs(0))
            }
        }
    }
}

// This is required to build an intrusive `RBTree` of `ConnectionNode`s which
// utilizes `ConnectionId`s as a key.
impl<'a, C: ConnectionTrait> KeyAdapter<'a> for ConnectionTreeAdapter<C> {
    type Key = InternalConnectionId;

    fn get_key(&self, node: &'a ConnectionNode<C>) -> InternalConnectionId {
        node.inner.borrow().internal_connection_id()
    }
}

/// Contains all secondary lists of Connections.
///
/// A Connection can be a member in any of those, in addition to being a member of
/// `ConnectionContainer::connection_map`.
struct InterestLists<C: ConnectionTrait> {
    /// Connections which have been finalized
    done_connections: LinkedList<DoneConnectionsAdapter<C>>,
    /// Connections which need to transmit data
    waiting_for_transmission: LinkedList<WaitingForTransmissionAdapter<C>>,
    /// Connections which need a new connection ID
    waiting_for_connection_id: LinkedList<WaitingForConnectionIdAdapter<C>>,
    /// Connections which are waiting for a timeout to occur
    waiting_for_timeout: RBTree<WaitingForTimeoutAdapter<C>>,
    /// Inflight handshake count
    handshake_connections: usize,
}

impl<C: ConnectionTrait> InterestLists<C> {
    fn new() -> Self {
        Self {
            done_connections: LinkedList::new(DoneConnectionsAdapter::new()),
            waiting_for_transmission: LinkedList::new(WaitingForTransmissionAdapter::new()),
            waiting_for_connection_id: LinkedList::new(WaitingForConnectionIdAdapter::new()),
            waiting_for_timeout: RBTree::new(WaitingForTimeoutAdapter::new()),
            handshake_connections: 0,
        }
    }

    /// Update all interest lists based on latest interest reported by a Node
    fn update_interests(
        &mut self,
        accept_queue: &mut unbounded_channel::Sender<Connection>,
        node: &Arc<ConnectionNode<C>>,
        interests: ConnectionInterests,
    ) {
        // Note that all comparisons start by checking whether the connection is
        // already part of the given list. This is required in order for the
        // following operation to be safe. Inserting an element in a list while
        // it is already part of a (different) list can panic. Trying to remove
        // an element from a list while it is not actually part of the list
        // is undefined.

        macro_rules! insert_interest {
            ($list_name:ident, $call:ident) => {
                // We have to obtain an `Arc<ConnectionNode>` in order to be able to
                // perform interest updates later on. However the intrusive tree
                // API only provides us a raw reference.
                // Safety: We know that all of our ConnectionNode's are stored in
                // reference counted pointers.
                let node = unsafe { node.arc_from_ref() };

                self.$list_name.$call(node);
            };
        }

        macro_rules! remove_interest {
            ($list_name:ident) => {
                // Safety: We know that the node is only ever part of this list.
                // While elements are in temporary lists, they always get unlinked
                // from those temporary lists while their interest is updated.
                let mut cursor = unsafe {
                    self.$list_name
                        .cursor_mut_from_ptr(node.deref() as *const ConnectionNode<C>)
                };
                cursor.remove();
            };
        }

        macro_rules! sync_interests_list {
            ($interest:expr, $link_name:ident, $list_name:ident) => {
                if $interest != node.$link_name.is_linked() {
                    if $interest {
                        insert_interest!($list_name, push_back);
                    } else {
                        remove_interest!($list_name);
                    }
                }
                debug_assert_eq!($interest, node.$link_name.is_linked());
            };
        }

        sync_interests_list!(
            interests.transmission,
            waiting_for_transmission_link,
            waiting_for_transmission
        );

        sync_interests_list!(
            interests.new_connection_id,
            waiting_for_connection_id_link,
            waiting_for_connection_id
        );

        // Check if the timeout has changed since last time we queried the interests
        if node.timeout.get() != interests.timeout {
            // remove the connection if it's currently linked
            if node.waiting_for_timeout_link.is_linked() {
                remove_interest!(waiting_for_timeout);
            }
            // set the new timeout value
            node.timeout.set(interests.timeout);
            // insert the connection if it still has a value
            if interests.timeout.is_some() {
                insert_interest!(waiting_for_timeout, insert);
            }
        } else {
            // make sure the timeout value reflects the connection's presense in the timeout list
            debug_assert_eq!(
                interests.timeout.is_some(),
                node.waiting_for_timeout_link.is_linked()
            );
        }

        // Accepted connections are only automatically pushed into the accepted connections queue.
        if interests.accept {
            // Mark the connection as accepted so that the interest gets reset. This is necessary
            // to avoid marking a connection as handed over twice.
            {
                let mut mutable_connection = node.inner.borrow_mut();
                mutable_connection.mark_as_accepted();
                // Decrement the inflight handshakes because this connection completed the
                // handshake and is being passed to the application to be accepted.
                self.handshake_connections -= 1;
            }

            if let State::Connected(shared_state) = &*node.shared_state.borrow() {
                if accept_queue
                    .send(Connection::new(shared_state.clone()))
                    .is_err()
                {
                    // TODO close the connection
                    return;
                }
            }
        }

        // Connections that enter the draining phase should have their shared state freed
        if interests.closing {
            *node.shared_state.borrow_mut() = State::Closing;
        }

        if interests.finalization != node.done_connections_link.is_linked() {
            if interests.finalization {
                self.done_connections.push_back(node.clone());
            } else {
                unreachable!("Done connections should never report not done later");
            }
        }
    }
}

/// A collection of all intrusive lists Connections are part of.
///
/// The container will automatically update the membership of a `Connection` in a
/// variety of interest lists after each interaction with the `Connection`.
///
/// The Connection container can be interacted with in 2 fashions:
/// - The `with_connection()` method allows users to obtain a mutable reference to
///   a single `Connection`. After the interaction was completed, the `Connection` will
///   be queried for its interests again.
/// - There exist a variety of iteration methods, which allow to iterate over
///   all or a subset of connections in each interest list.
pub struct ConnectionContainer<C: ConnectionTrait> {
    /// Connections organized as a tree, for lookup by Connection ID
    connection_map: RBTree<ConnectionTreeAdapter<C>>,
    /// Additional interest lists in which Connections will be placed dynamically
    interest_lists: InterestLists<C>,
    /// The synchronized queue of accepted connections
    accept_queue: unbounded_channel::Sender<Connection>,
}

macro_rules! iterate_interruptible {
    ($sel:ident, $list_name:tt, $link_name:ident, $func:ident) => {
        let mut extracted_list = $sel.interest_lists.$list_name.take();
        let mut cursor = extracted_list.front_mut();

        while let Some(connection) = cursor.remove() {
            // Note that while we iterate over the intrusive lists here
            // `Connection` is part of no list anymore, since it also got dropped
            // from list that is described by the `cursor`.
            debug_assert!(!connection.$link_name.is_linked());

            let (result, interests) = {
                let mut mut_connection = connection.inner.borrow_mut();

                let shared_state = connection.shared_state.borrow();
                let mut shared_state = shared_state.try_lock();

                let result = $func(&mut *mut_connection, shared_state.as_deref_mut());

                let interests = mut_connection.interests(shared_state.as_deref());
                (result, interests)
            };

            // Update the interests after the interaction and outside of the per-connection Mutex
            $sel.interest_lists
                .update_interests(&mut $sel.accept_queue, &connection, interests);

            match result {
                ConnectionContainerIterationResult::BreakAndInsertAtBack => {
                    $sel.interest_lists
                        .$list_name
                        .front_mut()
                        .splice_after(extracted_list);
                    break;
                }
                ConnectionContainerIterationResult::Continue => {}
            }
        }

        $sel.finalize_done_connections();
    };
}

impl<C: ConnectionTrait> ConnectionContainer<C> {
    /// Creates a new `ConnectionContainer`
    pub fn new(accept_queue: unbounded_channel::Sender<Connection>) -> Self {
        Self {
            connection_map: RBTree::new(ConnectionTreeAdapter::new()),
            interest_lists: InterestLists::new(),
            accept_queue,
        }
    }

    pub fn can_accept(&self) -> bool {
        self.accept_queue.is_open()
    }

    pub fn is_open(&self) -> bool {
        !self.connection_map.is_empty() || self.can_accept()
    }

    /// Returns the next `Timestamp` at which any contained connections will expire
    pub fn next_expiration(&self) -> Option<Timestamp> {
        let cursor = self.interest_lists.waiting_for_timeout.front();
        let node = cursor.get()?;
        let timeout = node.timeout.get();
        debug_assert!(
            timeout.is_some(),
            "a connection should only be in the timeout list when the timeout field is set"
        );
        timeout
    }

    /// Insert a new Connection into the container
    pub fn insert_connection(
        &mut self,
        connection: C,
        shared_state: Arc<SynchronizedSharedConnectionState<C::Config>>,
    ) {
        // Even though it likely might have none, it seems like it
        // would be better to avoid future bugs
        let interests = {
            let shared_state = &mut *shared_state.lock();
            connection.interests(Some(shared_state))
        };

        let new_connection = Arc::new(ConnectionNode::new(connection, shared_state));

        // Increment the inflight handshakes counter because we have accepted a new connection
        self.interest_lists.handshake_connections += 1;

        self.interest_lists
            .update_interests(&mut self.accept_queue, &new_connection, interests);
        self.connection_map.insert(new_connection);
        self.ensure_counter_consistency();
    }

    pub fn handshake_connections(&self) -> usize {
        self.interest_lists.handshake_connections
    }

    /// Looks up the `Connection` with the given ID and executes the provided function
    /// on it.
    ///
    /// After the transaction with the `Connection` had been completed, the `Connection`
    /// will get queried for its new interests, and all lists will be updated
    /// according to those.
    ///
    /// `Connection`s which signal finalization interest will be removed from the
    /// `ConnectionContainer`.
    pub fn with_connection<F, R>(
        &mut self,
        connection_id: InternalConnectionId,
        func: F,
    ) -> Option<(R, ConnectionInterests)>
    where
        F: FnOnce(&mut C, Option<&mut SharedConnectionState<C::Config>>) -> R,
    {
        let node_ptr: Arc<ConnectionNode<C>>;
        let result: R;
        let interests: ConnectionInterests;

        // This block is required since we mutably borrow `self` inside the
        // block in order to obtain a Connection reference and to executing the
        // provided method.
        // We need to release the borrow in order to be able to update the
        // Connections interests after having executed the method.
        {
            let node = self.connection_map.find(&connection_id).get()?;

            // We have to obtain an `Arc<ConnectionNode>` in order to be able to
            // perform interest updates later on. However the intrusive tree
            // API only provides us a raw reference.
            // Safety: We know that all of our ConnectionNode's are stored in
            // `Arc` pointers.
            node_ptr = unsafe { node.arc_from_ref() };

            // Lock the shared connection state
            let shared_state = node.shared_state.borrow();
            let mut shared_state = shared_state.try_lock();

            // Obtain a mutable reference to `Connection` implementation
            let connection: &mut C = &mut *node.inner.borrow_mut();

            result = func(connection, shared_state.as_deref_mut());

            interests = connection.interests(shared_state.as_deref());
        }

        // Update the interest lists after the interactions and outside of the per-connection Mutex.
        // Then remove all finalized connections
        self.interest_lists
            .update_interests(&mut self.accept_queue, &node_ptr, interests);
        self.ensure_counter_consistency();
        self.finalize_done_connections();

        Some((result, interests))
    }

    /// Removes all Connections in the `done` state from the `ConnectionContainer`.
    pub fn finalize_done_connections(&mut self) {
        for connection in self.interest_lists.done_connections.take() {
            // Remove the Connection from `connection_map`
            let mut cursor = self
                .connection_map
                .find_mut(&connection.inner.borrow().internal_connection_id());
            let remove_result = cursor.remove();
            debug_assert!(remove_result.is_some());

            // And remove the Connection from all other interest lists it might be
            // part of.
            let connection_ptr = &*connection as *const ConnectionNode<C>;

            macro_rules! remove_connection_from_list {
                ($list_name:ident, $link_name:ident) => {
                    if connection.$link_name.is_linked() {
                        // Safety: We know that the Connection is part of the list,
                        // because it is linked, and we never place Connections in
                        // other lists when `finalize_done_connections` is called.
                        let mut cursor = unsafe {
                            self.interest_lists
                                .$list_name
                                .cursor_mut_from_ptr(connection_ptr)
                        };
                        let remove_result = cursor.remove();
                        debug_assert!(remove_result.is_some());
                    }
                };
            }

            remove_connection_from_list!(waiting_for_transmission, waiting_for_transmission_link);
            remove_connection_from_list!(waiting_for_connection_id, waiting_for_connection_id_link);
            remove_connection_from_list!(waiting_for_timeout, waiting_for_timeout_link);

            // If the connection is still handshaking then it must have timed out.
            if connection.inner.borrow().is_handshaking() {
                self.interest_lists.handshake_connections -= 1;
                self.ensure_counter_consistency();
            }
        }
    }

    fn ensure_counter_consistency(&self) {
        if cfg!(debug_assertions) {
            let expected = self
                .connection_map
                .iter()
                .filter(|conn| conn.inner.borrow().is_handshaking())
                .count();
            assert_eq!(expected, self.interest_lists.handshake_connections);
        }
    }

    /// Iterates over all `Connection`s which are waiting for transmission,
    /// and executes the given function on each `Connection`
    pub fn iterate_transmission_list<F>(&mut self, mut func: F)
    where
        F: FnMut(
            &mut C,
            Option<&mut SharedConnectionState<C::Config>>,
        ) -> ConnectionContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_transmission,
            waiting_for_transmission_link,
            func
        );
    }

    /// Iterates over all `Connection`s which are waiting for new connection Ids,
    /// and executes the given function on each `Connection`
    pub fn iterate_new_connection_id_list<F>(&mut self, mut func: F)
    where
        F: FnMut(
            &mut C,
            Option<&mut SharedConnectionState<C::Config>>,
        ) -> ConnectionContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_connection_id,
            waiting_for_connection_id_link,
            func
        );
    }

    /// Iterates over all `Connection`s which are waiting for timeouts before the current time
    /// and executes the given function on each `Connection`
    pub fn iterate_timeout_list<F>(&mut self, now: Timestamp, mut func: F)
    where
        F: FnMut(&mut C, Option<&mut SharedConnectionState<C::Config>>),
    {
        loop {
            let mut cursor = self.interest_lists.waiting_for_timeout.front_mut();
            let connection = if let Some(connection) = cursor.get() {
                connection
            } else {
                break;
            };

            match connection.timeout.get() {
                Some(v) if !v.has_elapsed(now) => break,
                Some(_) => {}
                None => {
                    if cfg!(debug_assertions) {
                        panic!("connection was inserted without a timeout specified");
                    }

                    let conn = cursor.remove().unwrap();
                    conn.timeout.set(None);
                    continue;
                }
            }

            let connection = cursor
                .remove()
                .expect("list capacity was already checked in the `while` condition");

            // Note that while we iterate over the intrusive lists here
            // `Connection` is part of no list anymore, since it also got dropped
            // from list that is described by the `cursor`.
            debug_assert!(!connection.waiting_for_timeout_link.is_linked());
            // also clear the timer to make the state consistent
            connection.timeout.set(None);

            let mut interests = {
                // Lock the shared connection state
                let shared_state = connection.shared_state.borrow();
                let mut shared_state = shared_state.try_lock();

                // Obtain a mutable reference to `Connection` implementation
                let connection: &mut C = &mut *connection.inner.borrow_mut();

                func(connection, shared_state.as_deref_mut());

                connection.interests(shared_state.as_deref())
            };

            if let Some(timeout) = interests.timeout.as_mut() {
                // make sure the connection isn't trying to set a timer in the past
                if timeout.has_elapsed(now) {
                    // TODO panic with_debug_assertions once all of the connection components
                    //      are fixed to return times in the future

                    // fast forward the timer entry to the next granularity otherwise we'll
                    // endlessly loop here
                    *timeout = now + K_GRANULARITY;

                    // make sure that the new timeout wouldn't be considered elapsed
                    debug_assert!(!timeout.has_elapsed(now));
                }
            }

            // Update the interests after the interaction and outside of the per-connection Mutex
            self.interest_lists
                .update_interests(&mut self.accept_queue, &connection, interests);
        }

        self.finalize_done_connections();
    }
}

/// Return values for iterations over a `Coonnection` list.
/// The value intstructs the iterator whether iteration will be continued.
pub enum ConnectionContainerIterationResult {
    /// Continue iteration over the list
    Continue,
    /// Aborts the iteration over a list and add the remaining items at the
    /// back of the list
    BreakAndInsertAtBack,
}
