//! `ConnectionContainer` is a container for all Connections. It manages the permanent
//! map of all active Connections, as well as a variety of dynamic Connection lists.

use crate::{
    connection::{
        Connection, ConnectionInterests, InternalConnectionId, SharedConnectionState,
        SynchronizedSharedConnectionState, Trait as ConnectionTrait,
    },
    unbounded_channel,
};
use alloc::{rc::Rc, sync::Arc};
use core::{cell::RefCell, ops::Deref};
use intrusive_collections::{
    intrusive_adapter, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use s2n_quic_core::connection;

// Intrusive list adapter for managing the list of `done` connections
intrusive_adapter!(DoneConnectionsAdapter<C> = Rc<ConnectionNode<C>>: ConnectionNode<C> {
    done_connections_link: LinkedListLink
} where C: ConnectionTrait);

// Intrusive list adapter for managing the list of
// `waiting_for_transmission` connections
intrusive_adapter!(WaitingForTransmissionAdapter<C> = Rc<ConnectionNode<C>>: ConnectionNode<C> {
    waiting_for_transmission_link: LinkedListLink
} where C: ConnectionTrait);

// Intrusive list adapter for managing the list of
// `waiting_for_connection_id` connections
intrusive_adapter!(WaitingForConnectionIdAdapter<C> = Rc<ConnectionNode<C>>: ConnectionNode<C> {
    waiting_for_connection_id_link: LinkedListLink
} where C: ConnectionTrait);

// Intrusive red black tree adapter for managing all connections in a tree for
// lookup by Connection ID
intrusive_adapter!(ConnectionTreeAdapter<C> = Rc<ConnectionNode<C>>: ConnectionNode<C> {
    tree_link: RBTreeLink
} where C: ConnectionTrait);

/// A wrapper around a `Connection` implementation which allows to insert the
/// it in multiple intrusive collections. The collections into which the `Connection`
/// gets inserted are referenced inside this `ConnectionNode`.
struct ConnectionNode<C: ConnectionTrait> {
    /// This contains the actual implementation of the `Connection`
    inner: RefCell<C>,
    /// The shared state associated with the connection
    shared_state: Arc<SynchronizedSharedConnectionState<C::Config>>,
    /// Allows the Connection to be part of the `connection_map` collection
    tree_link: RBTreeLink,
    /// Allows the Connection to be part of the `done_connections` collection
    done_connections_link: LinkedListLink,
    #[allow(dead_code)] // TODO do we still need this?
    /// Allows the Connection to be part of the `accepted_connections` collection
    accepted_connections_link: LinkedListLink,
    /// Allows the Connection to be part of the `waiting_for_transmission` collection
    waiting_for_transmission_link: LinkedListLink,
    /// Allows the Connection to be part of the `waiting_for_connection_id` collection
    waiting_for_connection_id_link: LinkedListLink,
}

impl<C: ConnectionTrait> ConnectionNode<C> {
    /// Creates a new `ConnectionNode` which wraps the given Connection implementation of type `S`
    pub fn new(
        connection_impl: C,
        shared_state: Arc<SynchronizedSharedConnectionState<C::Config>>,
    ) -> ConnectionNode<C> {
        ConnectionNode {
            inner: RefCell::new(connection_impl),
            shared_state,
            tree_link: RBTreeLink::new(),
            accepted_connections_link: LinkedListLink::new(),
            done_connections_link: LinkedListLink::new(),
            waiting_for_transmission_link: LinkedListLink::new(),
            waiting_for_connection_id_link: LinkedListLink::new(),
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

/// Obtains a `Rc<ConnectionNode>` from a `&ConnectionNode`.
///
/// This method is only safe to be called if the `ConnectionNode` is known to be
/// stored inside a `Rc`.
unsafe fn connnection_node_rc_from_ref<C: ConnectionTrait>(
    connection_node: &ConnectionNode<C>,
) -> Rc<ConnectionNode<C>> {
    // In order to be able to to get a `Rc` we construct a temporary `Rc`
    // from it using the `Rc::from_raw` API and clone the `Rc`.
    // The temporary `Rc` must be released without calling `drop`,
    // because this would decrement and thereby invalidate the refcount
    // (which wasn't changed by calling `Rc::from_raw`).
    let temp_node_ptr: core::mem::ManuallyDrop<Rc<ConnectionNode<C>>> =
        core::mem::ManuallyDrop::new(Rc::<ConnectionNode<C>>::from_raw(
            connection_node as *const ConnectionNode<C>,
        ));
    temp_node_ptr.deref().clone()
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
}

impl<C: ConnectionTrait> InterestLists<C> {
    fn new() -> Self {
        Self {
            done_connections: LinkedList::new(DoneConnectionsAdapter::new()),
            waiting_for_transmission: LinkedList::new(WaitingForTransmissionAdapter::new()),
            waiting_for_connection_id: LinkedList::new(WaitingForConnectionIdAdapter::new()),
        }
    }

    /// Update all interest lists based on latest interest reported by a Node
    fn update_interests(
        &mut self,
        accept_queue: &mut unbounded_channel::Sender<Connection>,
        node: &Rc<ConnectionNode<C>>,
        interests: ConnectionInterests,
    ) {
        // Note that all comparisons start by checking whether the connection is
        // already part of the given list. This is required in order for the
        // following operation to be safe. Inserting an element in a list while
        // it is already part of a (different) list can panic. Trying to remove
        // an element from a list while it is not actually part of the list
        // is undefined.

        macro_rules! sync_interests {
            ($interest:expr, $link_name:ident, $list_name:ident) => {
                if $interest != node.$link_name.is_linked() {
                    if $interest {
                        self.$list_name.push_back(node.clone());
                    } else {
                        // Safety: We know that the node is only ever part of this list.
                        // While elements are in temporary lists, they always get unlinked
                        // from those temporary lists while their interest is updated.
                        let mut cursor = unsafe {
                            self.$list_name
                                .cursor_mut_from_ptr(node.deref() as *const ConnectionNode<C>)
                        };
                        cursor.remove();
                    }
                }
                debug_assert_eq!($interest, node.$link_name.is_linked());
            };
        }

        sync_interests!(
            interests.transmission,
            waiting_for_transmission_link,
            waiting_for_transmission
        );

        let has_connection_id_interest = interests.id != connection::id::Interest::None;
        sync_interests!(
            has_connection_id_interest,
            waiting_for_connection_id_link,
            waiting_for_connection_id
        );

        // Accepted connections are only automatically pushed into the accepted connections queue.
        if interests.accept {
            // Mark the connection as accepted so that the interest gets reset. This is necessary
            // to avoid marking a connection as handed over twice.
            {
                let mut mutable_connection = node.inner.borrow_mut();
                mutable_connection.mark_as_accepted();
            }

            // TODO shutdown endpoint if we can't send connections
            if accept_queue
                .send(Connection::new(node.shared_state.clone()))
                .is_err()
            {
                return;
            }
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
                let shared_state = &mut *connection.shared_state.lock();

                let result = $func(&mut *mut_connection, shared_state);
                mut_connection.update_connection_timer(shared_state);
                let interests = mut_connection.interests(shared_state);
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

    /// Insert a new Connection into the container
    pub fn insert_connection(
        &mut self,
        mut connection: C,
        shared_state: Arc<SynchronizedSharedConnectionState<C::Config>>,
    ) {
        // Even though it likely might have none, it seems like it
        // would be better to avoid future bugs
        let interests = {
            let shared_state = &mut *shared_state.lock();
            connection.update_connection_timer(shared_state);
            connection.interests(shared_state)
        };

        let new_connection = Rc::new(ConnectionNode::new(connection, shared_state));

        self.interest_lists
            .update_interests(&mut self.accept_queue, &new_connection, interests);
        self.connection_map.insert(new_connection);
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
        F: FnOnce(&mut C, &mut SharedConnectionState<C::Config>) -> R,
    {
        let node_ptr: Rc<ConnectionNode<C>>;
        let result: R;
        let interests: ConnectionInterests;

        // This block is required since we mutably borrow `self` inside the
        // block in order to obtain a Connection reference and to executing the
        // provided method.
        // We need to release the borrow in order to be able to update the
        // Connections interests after having executed the method.
        {
            let node = self.connection_map.find(&connection_id).get()?;

            // We have to obtain an `Rc<ConnectionNode>` in order to be able to
            // perform interest updates later on. However the intrusive tree
            // API only provides us a raw reference.
            // Safety: We know that all of our ConnectionNode's are stored in
            // `Rc` pointers.
            node_ptr = unsafe { connnection_node_rc_from_ref(node) };

            // Lock the shared connection state
            let shared_state = &mut *node.shared_state.lock();

            // Obtain a mutable reference to `Connection` implementation
            let connection: &mut C = &mut *node.inner.borrow_mut();
            result = func(connection, shared_state);
            connection.update_connection_timer(shared_state);
            interests = connection.interests(shared_state);
        }

        // Update the interest lists after the interactions and outside of the per-connection Mutex.
        // Then remove all finalized connections
        self.interest_lists
            .update_interests(&mut self.accept_queue, &node_ptr, interests);
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
        }
    }

    /// Iterates over all `Connection`s which are waiting for transmission,
    /// and executes the given function on each `Connection`
    pub fn iterate_transmission_list<F>(&mut self, mut func: F)
    where
        F: FnMut(
            &mut C,
            &mut SharedConnectionState<C::Config>,
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
            &mut SharedConnectionState<C::Config>,
        ) -> ConnectionContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_connection_id,
            waiting_for_connection_id_link,
            func
        );
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
