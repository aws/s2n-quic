// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `ConnectionContainer` is a container for all Connections. It manages the permanent
//! map of all active Connections, as well as a variety of dynamic Connection lists.

use super::{ConnectionApi, ConnectionApiProvider};
use crate::{
    connection::{self, Connection, ConnectionInterests, InternalConnectionId},
    stream, unbounded_channel,
};
use alloc::sync::Arc;
use bytes::Bytes;
use core::{
    cell::Cell,
    marker::PhantomData,
    ops::Deref,
    task::{Context, Poll},
};
use intrusive_collections::{
    intrusive_adapter, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use s2n_quic_core::{application, inet::SocketAddress, recovery::K_GRANULARITY, time::Timestamp};

// Intrusive list adapter for managing the list of `done` connections
intrusive_adapter!(DoneConnectionsAdapter<C, L> = Arc<ConnectionNode<C, L>>: ConnectionNode<C, L> {
    done_connections_link: LinkedListLink
} where C: connection::Trait, L: connection::Lock<C>);

// Intrusive list adapter for managing the list of
// `waiting_for_transmission` connections
intrusive_adapter!(WaitingForTransmissionAdapter<C, L> = Arc<ConnectionNode<C, L>>: ConnectionNode<C, L> {
    waiting_for_transmission_link: LinkedListLink
} where C: connection::Trait, L: connection::Lock<C>);

// Intrusive list adapter for managing the list of
// `waiting_for_connection_id` connections
intrusive_adapter!(WaitingForConnectionIdAdapter<C, L> = Arc<ConnectionNode<C, L>>: ConnectionNode<C, L> {
    waiting_for_connection_id_link: LinkedListLink
} where C: connection::Trait, L: connection::Lock<C>);

// Intrusive red black tree adapter for managing a list of `waiting_for_timeout` connections
intrusive_adapter!(WaitingForTimeoutAdapter<C, L> = Arc<ConnectionNode<C, L>>: ConnectionNode<C, L> {
    waiting_for_timeout_link: RBTreeLink
} where C: connection::Trait, L: connection::Lock<C>);

// Intrusive red black tree adapter for managing all connections in a tree for
// lookup by Connection ID
intrusive_adapter!(ConnectionTreeAdapter<C, L> = Arc<ConnectionNode<C, L>>: ConnectionNode<C, L> {
    tree_link: RBTreeLink
} where C: connection::Trait, L: connection::Lock<C>);

/// A wrapper around a `Connection` implementation which allows to insert the
/// it in multiple intrusive collections. The collections into which the `Connection`
/// gets inserted are referenced inside this `ConnectionNode`.
struct ConnectionNode<C: connection::Trait, L: connection::Lock<C>> {
    /// This contains the actual implementation of the `Connection`
    inner: L,
    /// The connection id pertaining to the stored connection
    internal_connection_id: InternalConnectionId,
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
    /// The inner connection type
    _connection: PhantomData<C>,
}

impl<C: connection::Trait, L: connection::Lock<C>> ConnectionNode<C, L> {
    /// Creates a new `ConnectionNode` which wraps the given Connection implementation
    pub fn new(
        connection_impl: L,
        internal_connection_id: InternalConnectionId,
    ) -> ConnectionNode<C, L> {
        ConnectionNode {
            inner: connection_impl,
            internal_connection_id,
            tree_link: RBTreeLink::new(),
            done_connections_link: LinkedListLink::new(),
            waiting_for_transmission_link: LinkedListLink::new(),
            waiting_for_connection_id_link: LinkedListLink::new(),
            waiting_for_timeout_link: RBTreeLink::new(),
            timeout: Cell::new(None),
            _connection: PhantomData,
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
        let temp_node_ptr: core::mem::ManuallyDrop<Arc<ConnectionNode<C, L>>> =
            core::mem::ManuallyDrop::new(Arc::<ConnectionNode<C, L>>::from_raw(
                self as *const ConnectionNode<C, L>,
            ));

        temp_node_ptr.deref().clone()
    }

    /// Performs an application API write call that returns a connection result
    fn api_write_call<F: FnOnce(&mut C) -> Result<R, E>, R, E: From<connection::Error>>(
        &self,
        f: F,
    ) -> Result<R, E> {
        match self.inner.write(|conn| f(conn)) {
            Ok(res) => res,
            Err(_) => Err(connection::Error::Unspecified.into()),
        }
    }

    /// Performs an application API read call that returns a connection result
    fn api_read_call<F: FnOnce(&C) -> Result<R, E>, R, E: From<connection::Error>>(
        &self,
        f: F,
    ) -> Result<R, E> {
        match self.inner.read(|conn| f(conn)) {
            Ok(res) => res,
            Err(_) => Err(connection::Error::Unspecified.into()),
        }
    }

    /// Performs an application API call that returns a poll outcome
    fn api_poll_call<F: FnOnce(&mut C) -> Poll<Result<R, E>>, R, E: From<connection::Error>>(
        &self,
        f: F,
    ) -> Poll<Result<R, E>> {
        match self.inner.write(|conn| f(conn)) {
            Ok(res) => res,
            Err(_) => Poll::Ready(Err(connection::Error::Unspecified.into())),
        }
    }
}

impl<'a, C: connection::Trait, L: connection::Lock<C>> KeyAdapter<'a>
    for WaitingForTimeoutAdapter<C, L>
{
    type Key = Timestamp;

    fn get_key(&self, node: &'a ConnectionNode<C, L>) -> Timestamp {
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
impl<'a, C: connection::Trait, L: connection::Lock<C>> KeyAdapter<'a>
    for ConnectionTreeAdapter<C, L>
{
    type Key = InternalConnectionId;

    fn get_key(&self, node: &'a ConnectionNode<C, L>) -> InternalConnectionId {
        node.internal_connection_id
    }
}

/// Safety: ConnectionNode uses connection::Lock to ensure all cross-thread access is synchronized
unsafe impl<C: connection::Trait, L: connection::Lock<C>> Sync for ConnectionNode<C, L> {}

impl<C: connection::Trait, L: connection::Lock<C>> ConnectionApiProvider for ConnectionNode<C, L> {
    fn poll_request(
        &self,
        stream_id: stream::StreamId,
        request: &mut stream::ops::Request,
        context: Option<&Context>,
    ) -> Result<stream::ops::Response, stream::StreamError> {
        self.api_write_call(|conn| conn.poll_stream_request(stream_id, request, context))
    }

    fn poll_accept(
        &self,
        arc_self: &ConnectionApi,
        stream_type: Option<stream::StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<stream::Stream>, connection::Error>> {
        let response = self.api_poll_call(|conn| conn.poll_accept_stream(stream_type, context));

        match response {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Err(e).into(),
            Poll::Ready(Ok(None)) => Ok(None).into(),
            Poll::Ready(Ok(Some(stream_id))) => {
                let stream = stream::Stream::new(arc_self.clone(), stream_id);

                Ok(Some(stream)).into()
            }
        }
    }

    fn poll_open_stream(
        &self,
        arc_self: &ConnectionApi,
        stream_type: stream::StreamType,
        context: &Context,
    ) -> Poll<Result<stream::Stream, connection::Error>> {
        let response = self.api_poll_call(|conn| conn.poll_open_stream(stream_type, context));

        match response {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Err(e).into(),
            Poll::Ready(Ok(stream_id)) => {
                let stream = stream::Stream::new(arc_self.clone(), stream_id);

                Ok(stream).into()
            }
        }
    }

    fn close_connection(&self, error: Option<application::Error>) {
        let _: Result<(), connection::Error> = self.api_write_call(|conn| {
            conn.application_close(error);
            Ok(())
        });
    }

    fn sni(&self) -> Result<Option<Bytes>, connection::Error> {
        self.api_read_call(|conn| Ok(conn.sni()))
    }

    fn alpn(&self) -> Result<Bytes, connection::Error> {
        self.api_read_call(|conn| Ok(conn.alpn()))
    }

    fn id(&self) -> u64 {
        self.internal_connection_id.into()
    }

    fn ping(&self) -> Result<(), connection::Error> {
        self.api_write_call(|conn| conn.ping())
    }

    fn local_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api_read_call(|conn| conn.local_address())
    }

    fn remote_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api_read_call(|conn| conn.remote_address())
    }
}

/// Contains all secondary lists of Connections.
///
/// A Connection can be a member in any of those, in addition to being a member of
/// `ConnectionContainer::connection_map`.
struct InterestLists<C: connection::Trait, L: connection::Lock<C>> {
    /// Connections which have been finalized
    done_connections: LinkedList<DoneConnectionsAdapter<C, L>>,
    /// Connections which need to transmit data
    waiting_for_transmission: LinkedList<WaitingForTransmissionAdapter<C, L>>,
    /// Connections which need a new connection ID
    waiting_for_connection_id: LinkedList<WaitingForConnectionIdAdapter<C, L>>,
    /// Connections which are waiting for a timeout to occur
    waiting_for_timeout: RBTree<WaitingForTimeoutAdapter<C, L>>,
    /// Inflight handshake count
    handshake_connections: usize,
}

impl<C: connection::Trait, L: connection::Lock<C>> InterestLists<C, L> {
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
        node: &ConnectionNode<C, L>,
        interests: ConnectionInterests,
    ) -> Result<(), L::Error> {
        // Note that all comparisons start by checking whether the connection is
        // already part of the given list. This is required in order for the
        // following operation to be safe. Inserting an element in a list while
        // it is already part of a (different) list can panic. Trying to remove
        // an element from a list while it is not actually part of the list
        // is undefined.

        macro_rules! insert_interest {
            ($list_name:ident, $call:ident) => {
                let node = unsafe {
                    // We have to obtain an `Arc<ConnectionNode>` in order to be able to
                    // perform interest updates later on. However the intrusive tree
                    // API only provides us a raw reference.
                    // Safety: We know that all of our ConnectionNode's are stored in
                    // reference counted pointers.
                    node.arc_from_ref()
                };

                self.$list_name.$call(node);
            };
        }

        macro_rules! remove_interest {
            ($list_name:ident) => {
                let mut cursor = unsafe {
                    // Safety: We know that the node is only ever part of this list.
                    // While elements are in temporary lists, they always get unlinked
                    // from those temporary lists while their interest is updated.
                    self.$list_name
                        .cursor_mut_from_ptr(node.deref() as *const ConnectionNode<C, L>)
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
            node.inner.write(|conn| {
                debug_assert!(!conn.is_handshaking());
                conn.mark_as_accepted()
            })?;

            // Decrement the inflight handshakes because this connection completed the
            // handshake and is being passed to the application to be accepted.
            self.handshake_connections -= 1;

            let handle = unsafe {
                // We have to obtain an `Arc<ConnectionNode>` in order to be able to
                // perform interest updates later on. However the intrusive tree
                // API only provides us a raw reference.
                // Safety: We know that all of our ConnectionNode's are stored in
                // reference counted pointers.
                node.arc_from_ref()
            };
            let handle = crate::connection::api::Connection::new(handle);

            if let Err((handle, _)) = accept_queue.send(handle) {
                handle.api.close_connection(None);
            }
        }

        if interests.finalization != node.done_connections_link.is_linked() {
            if interests.finalization {
                insert_interest!(done_connections, push_back);
            } else {
                unreachable!("Done connections should never report not done later");
            }
        }

        Ok(())
    }

    fn remove_node(&mut self, connection: &ConnectionNode<C, L>) {
        // And remove the Connection from all other interest lists it might be
        // part of.
        let connection_ptr = &*connection as *const ConnectionNode<C, L>;

        macro_rules! remove_connection_from_list {
            ($list_name:ident, $link_name:ident) => {
                if connection.$link_name.is_linked() {
                    let mut cursor = unsafe {
                        // Safety: We know that the Connection is part of the list,
                        // because it is linked, and we never place Connections in
                        // other lists when `finalize_done_connections` is called.
                        self.$list_name.cursor_mut_from_ptr(connection_ptr)
                    };
                    let remove_result = cursor.remove();
                    debug_assert!(remove_result.is_some());
                }
            };
        }

        remove_connection_from_list!(waiting_for_transmission, waiting_for_transmission_link);
        remove_connection_from_list!(waiting_for_connection_id, waiting_for_connection_id_link);
        remove_connection_from_list!(waiting_for_timeout, waiting_for_timeout_link);
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
pub struct ConnectionContainer<C: connection::Trait, L: connection::Lock<C>> {
    /// Connections organized as a tree, for lookup by Connection ID
    connection_map: RBTree<ConnectionTreeAdapter<C, L>>,
    /// Additional interest lists in which Connections will be placed dynamically
    interest_lists: InterestLists<C, L>,
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

            let (result, interests) = match connection.inner.write(|conn| {
                let result = $func(conn);
                let interests = conn.interests();
                (result, interests)
            }) {
                Ok(result) => result,
                Err(_) => {
                    // the connection panicked so remove it from the container
                    $sel.remove_poisoned_node(&connection);
                    continue;
                }
            };

            // Update the interests after the interaction and outside of the per-connection Mutex
            if $sel
                .interest_lists
                .update_interests(&mut $sel.accept_queue, &connection, interests)
                .is_err()
            {
                $sel.remove_poisoned_node(&connection);
            }

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

impl<C: connection::Trait, L: connection::Lock<C>> ConnectionContainer<C, L> {
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
        internal_connection_id: InternalConnectionId,
    ) {
        let interests = connection.interests();

        let connection = L::new(connection);
        let connection = Arc::new(ConnectionNode::new(connection, internal_connection_id));

        // Increment the inflight handshakes counter because we have accepted a new connection
        self.interest_lists.handshake_connections += 1;

        if self
            .interest_lists
            .update_interests(&mut self.accept_queue, &connection, interests)
            .is_ok()
        {
            self.connection_map.insert(connection);
            self.ensure_counter_consistency();
        }
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
        F: FnOnce(&mut C) -> R,
    {
        let cursor = self.connection_map.find(&connection_id);
        let node = cursor.get()?;

        let (result, interests) = match node.inner.write(|conn| {
            let result = func(conn);
            let interests = conn.interests();
            (result, interests)
        }) {
            Ok(result) => result,
            Err(_) => {
                // the connection panicked so remove it from the container
                let id = node.internal_connection_id;
                self.remove_node_by_id(id);
                self.interest_lists.handshake_connections = self.count_handshaking_connections();
                return None;
            }
        };

        // Update the interest lists after the interactions and outside of the per-connection Mutex.
        // Then remove all finalized connections
        if self
            .interest_lists
            .update_interests(&mut self.accept_queue, node, interests)
            .is_err()
        {
            let id = node.internal_connection_id;
            self.remove_node_by_id(id);
            self.interest_lists.handshake_connections = self.count_handshaking_connections();
        }

        self.ensure_counter_consistency();
        self.finalize_done_connections();

        Some((result, interests))
    }

    /// Removes all Connections in the `done` state from the `ConnectionContainer`.
    pub fn finalize_done_connections(&mut self) {
        for connection in self.interest_lists.done_connections.take() {
            self.remove_node(&connection);

            // If the connection is still handshaking then it must have timed out.
            let result = connection.inner.read(|conn| conn.is_handshaking());
            match result {
                Ok(true) => {
                    self.interest_lists.handshake_connections -= 1;
                    self.ensure_counter_consistency();
                }
                Ok(false) => {
                    // nothing to do
                }
                Err(_) => {
                    // The connection panicked so we need to recompute all of the handshaking
                    // connections
                    self.interest_lists.handshake_connections =
                        self.count_handshaking_connections();
                }
            }
        }
    }

    fn count_handshaking_connections(&self) -> usize {
        self.connection_map
            .iter()
            .filter(|conn| {
                conn.inner
                    .read(|conn| conn.is_handshaking())
                    .ok()
                    .unwrap_or(false)
            })
            .count()
    }

    fn ensure_counter_consistency(&self) {
        if cfg!(debug_assertions) {
            let expected = self.count_handshaking_connections();
            assert_eq!(expected, self.interest_lists.handshake_connections);
        }
    }

    /// Iterates over all `Connection`s which are waiting for transmission,
    /// and executes the given function on each `Connection`
    pub fn iterate_transmission_list<F>(&mut self, mut func: F)
    where
        F: FnMut(&mut C) -> ConnectionContainerIterationResult,
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
        F: FnMut(&mut C) -> ConnectionContainerIterationResult,
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
        F: FnMut(&mut C),
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

            let mut interests = match connection.inner.write(|conn| {
                func(conn);
                conn.interests()
            }) {
                Ok(result) => result,
                Err(_) => {
                    self.remove_poisoned_node(&connection);
                    continue;
                }
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
            if self
                .interest_lists
                .update_interests(&mut self.accept_queue, &connection, interests)
                .is_err()
            {
                self.remove_poisoned_node(&connection);
            }
        }

        self.finalize_done_connections();
    }

    fn remove_node_by_id(&mut self, connection_id: InternalConnectionId) {
        // Remove the Connection from `connection_map`
        let mut cursor = self.connection_map.find_mut(&connection_id);
        let remove_result = cursor.remove();
        debug_assert!(remove_result.is_some());

        if let Some(connection) = remove_result {
            self.interest_lists.remove_node(&connection);
        }
    }

    fn remove_poisoned_node(&mut self, connection: &ConnectionNode<C, L>) {
        // the connection panicked so remove it from the container
        self.remove_node(connection);

        // The connection panicked so we need to recompute all of the handshaking
        // connections since we don't know if it was previously handshaking or not
        self.interest_lists.handshake_connections = self.count_handshaking_connections();
    }

    fn remove_node(&mut self, connection: &ConnectionNode<C, L>) {
        // Remove the Connection from `connection_map`
        let mut cursor = self
            .connection_map
            .find_mut(&connection.internal_connection_id);
        let remove_result = cursor.remove();
        debug_assert!(remove_result.is_some());

        self.interest_lists.remove_node(connection);
    }
}

/// Return values for iterations over a `Connection` list.
/// The value instructs the iterator whether iteration will be continued.
pub enum ConnectionContainerIterationResult {
    /// Continue iteration over the list
    Continue,
    /// Aborts the iteration over a list and add the remaining items at the
    /// back of the list
    BreakAndInsertAtBack,
}

#[cfg(test)]
mod tests;
