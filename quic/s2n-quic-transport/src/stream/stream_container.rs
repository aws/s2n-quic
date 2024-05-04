// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `StreamContainer` is a container for all Streams. It manages the permanent
//! map of all active Streams, as well as a variety of dynamic Stream lists.

// hide warnings from the intrusive_collections crate
#![allow(unknown_lints, clippy::non_send_fields_in_send_ty)]

use crate::{
    stream,
    stream::{stream_impl::StreamTrait, stream_interests::StreamInterests},
    transmission,
};
use alloc::rc::Rc;
use core::{cell::RefCell, ops::Deref};
use intrusive_collections::{
    intrusive_adapter, KeyAdapter, LinkedList, LinkedListLink, RBTree, RBTreeLink,
};
use s2n_quic_core::{stream::StreamId, time::timer};

// Intrusive list adapter for managing the list of `done` streams
intrusive_adapter!(DoneStreamsAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    done_streams_link: LinkedListLink
});

// Intrusive list adapter for managing the list of `waiting_for_frame_delivery` streams
intrusive_adapter!(WaitingForFrameDeliveryAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    waiting_for_frame_delivery_link: LinkedListLink
});

// Intrusive list adapter for managing the list of
// `waiting_for_transmission` streams
intrusive_adapter!(WaitingForTransmissionAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    waiting_for_transmission_link: LinkedListLink
});

// Intrusive list adapter for managing the list of
// `waiting_for_retransmission` streams
intrusive_adapter!(WaitingForRetransmissionAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    waiting_for_retransmission_link: LinkedListLink
});

// Intrusive list adapter for managing the list of
// `waiting_for_connection_flow_control_credits` streams
intrusive_adapter!(WaitingForConnectionFlowControlCreditsAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    waiting_for_connection_flow_control_credits_link: LinkedListLink
});

// Intrusive list adapter for managing the list of
// `waiting_for_stream_flow_control_credits` streams
intrusive_adapter!(WaitingForStreamFlowControlCreditsAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    waiting_for_stream_flow_control_credits_link: LinkedListLink
});

// Intrusive red black tree adapter for managing all streams in a tree for
// lookup by Stream ID
intrusive_adapter!(StreamTreeAdapter<S> = Rc<StreamNode<S>>: StreamNode<S> {
    tree_link: RBTreeLink
});

/// A wrapper around a `Stream` implementation which allows to insert the
/// it in multiple intrusive collections. The collections into which the `Stream`
/// gets inserted are referenced inside this `StreamNode`.
struct StreamNode<S> {
    /// This contains the actual implementation of the `Stream`
    inner: RefCell<S>,
    /// Allows the Stream to be part of the `stream_map` collection
    tree_link: RBTreeLink,
    /// Allows the Stream to be part of the `done_streams` collection
    done_streams_link: LinkedListLink,
    /// Allows the Stream to be part of the `waiting_for_frame_delivery` collection
    waiting_for_frame_delivery_link: LinkedListLink,
    /// Allows the Stream to be part of the `waiting_for_transmission` collection
    waiting_for_transmission_link: LinkedListLink,
    /// Allows the Stream to be part of the `waiting_for_transmission` collection
    waiting_for_retransmission_link: LinkedListLink,
    /// Allows the Stream to be part of the `waiting_for_connection_flow_control_credits` collection
    waiting_for_connection_flow_control_credits_link: LinkedListLink,
    /// Allows the Stream to be part of the `waiting_for_stream_flow_control_credits` collection
    waiting_for_stream_flow_control_credits_link: LinkedListLink,
}

impl<S> StreamNode<S> {
    /// Creates a new `StreamNode` which wraps the given Stream implementation of type `S`
    pub fn new(stream_impl: S) -> StreamNode<S> {
        StreamNode {
            inner: RefCell::new(stream_impl),
            tree_link: RBTreeLink::new(),
            done_streams_link: LinkedListLink::new(),
            waiting_for_frame_delivery_link: LinkedListLink::new(),
            waiting_for_transmission_link: LinkedListLink::new(),
            waiting_for_retransmission_link: LinkedListLink::new(),
            waiting_for_connection_flow_control_credits_link: LinkedListLink::new(),
            waiting_for_stream_flow_control_credits_link: LinkedListLink::new(),
        }
    }
}

// This is required to build an intrusive `RBTree` of `StreamNode`s which
// utilizes `StreamId`s as a key.
impl<'a, S: StreamTrait> KeyAdapter<'a> for StreamTreeAdapter<S> {
    type Key = StreamId;

    fn get_key(&self, x: &'a StreamNode<S>) -> StreamId {
        x.inner.borrow().stream_id()
    }
}

/// Obtains a `Rc<StreamNode>` from a `&StreamNode`.
///
/// This method is only safe to be called if the `StreamNode` is known to be
/// stored inside a `Rc`.
unsafe fn stream_node_rc_from_ref<S>(stream_node: &StreamNode<S>) -> Rc<StreamNode<S>> {
    // In order to be able to to get a `Rc` we construct a temporary `Rc`
    // from it using the `Rc::from_raw` API and clone the `Rc`.
    // The temporary `Rc` must be released without calling `drop`,
    // because this would decrement and thereby invalidate the refcount
    // (which wasn't changed by calling `Rc::from_raw`).
    let temp_node_ptr: core::mem::ManuallyDrop<Rc<StreamNode<S>>> = core::mem::ManuallyDrop::new(
        Rc::<StreamNode<S>>::from_raw(stream_node as *const StreamNode<S>),
    );
    temp_node_ptr.deref().clone()
}

/// Contains all secondary lists of Streams.
///
/// A Stream can be a member in any of those, in addition to being a member of
/// `StreamContainer::stream_map`.
struct InterestLists<S> {
    /// Streams which have been finalized
    done_streams: LinkedList<DoneStreamsAdapter<S>>,
    /// Streams which are waiting for packet acknowledgements and
    /// packet loss notifications
    waiting_for_frame_delivery: LinkedList<WaitingForFrameDeliveryAdapter<S>>,
    /// Streams which need to transmit data
    waiting_for_transmission: LinkedList<WaitingForTransmissionAdapter<S>>,
    /// Streams which need to transmit data
    waiting_for_retransmission: LinkedList<WaitingForRetransmissionAdapter<S>>,
    /// Streams which are blocked on transmission due to waiting on the
    /// connection flow control window to increase
    waiting_for_connection_flow_control_credits:
        LinkedList<WaitingForConnectionFlowControlCreditsAdapter<S>>,
    /// Streams which are blocked on transmission due to waiting on the
    /// stream flow control window to increase
    waiting_for_stream_flow_control_credits:
        LinkedList<WaitingForStreamFlowControlCreditsAdapter<S>>,
}

impl<S: StreamTrait> InterestLists<S> {
    fn new() -> Self {
        Self {
            done_streams: LinkedList::new(DoneStreamsAdapter::new()),
            waiting_for_frame_delivery: LinkedList::new(WaitingForFrameDeliveryAdapter::new()),
            waiting_for_transmission: LinkedList::new(WaitingForTransmissionAdapter::new()),
            waiting_for_retransmission: LinkedList::new(WaitingForRetransmissionAdapter::new()),
            waiting_for_connection_flow_control_credits: LinkedList::new(
                WaitingForConnectionFlowControlCreditsAdapter::new(),
            ),
            waiting_for_stream_flow_control_credits: LinkedList::new(
                WaitingForStreamFlowControlCreditsAdapter::new(),
            ),
        }
    }

    /// Update all interest lists based on latest interest reported by a Node
    fn update_interests(
        &mut self,
        node: &Rc<StreamNode<S>>,
        interests: StreamInterests,
        result: StreamContainerIterationResult,
    ) -> bool {
        // Note that all comparisons start by checking whether the stream is
        // already part of the given list. This is required in order for the
        // following operation to be safe. Inserting an element in a list while
        // it is already part of a (different) list can panic. Trying to remove
        // an element from a list while it is not actually part of the list
        // is undefined.

        macro_rules! sync_interests {
            ($interest:expr, $link_name:ident, $list_name:ident) => {
                if $interest != node.$link_name.is_linked() {
                    if $interest {
                        if matches!(result, StreamContainerIterationResult::Continue) {
                            self.$list_name.push_back(node.clone());
                        } else {
                            self.$list_name.push_front(node.clone());
                        }
                    } else {
                        // Safety: We know that the node is only ever part of this list.
                        // While elements are in temporary lists, they always get unlinked
                        // from those temporary lists while their interest is updated.
                        let mut cursor = unsafe {
                            self.$list_name
                                .cursor_mut_from_ptr(node.deref() as *const StreamNode<S>)
                        };
                        cursor.remove();
                    }
                }
                debug_assert_eq!($interest, node.$link_name.is_linked());
            };
        }

        sync_interests!(
            interests.delivery_notifications,
            waiting_for_frame_delivery_link,
            waiting_for_frame_delivery
        );
        sync_interests!(
            matches!(interests.transmission, transmission::Interest::NewData),
            waiting_for_transmission_link,
            waiting_for_transmission
        );
        sync_interests!(
            matches!(interests.transmission, transmission::Interest::LostData),
            waiting_for_retransmission_link,
            waiting_for_retransmission
        );
        sync_interests!(
            interests.connection_flow_control_credits,
            waiting_for_connection_flow_control_credits_link,
            waiting_for_connection_flow_control_credits
        );
        sync_interests!(
            interests.stream_flow_control_credits,
            waiting_for_stream_flow_control_credits_link,
            waiting_for_stream_flow_control_credits
        );

        if interests.retained == node.done_streams_link.is_linked() {
            if !interests.retained {
                self.done_streams.push_back(node.clone());
            } else {
                panic!("Done streams should never report not done later");
            }
            true
        } else {
            false
        }
    }
}

/// A collection of all intrusive lists Streams are part of.
///
/// The container will automatically update the membership of a `Stream` in a
/// variety of interest lists after each interaction with the `Stream`.
///
/// The Stream container can be interacted with in 2 fashions:
/// - The `with_stream()` method allows users to obtain a mutable reference to
///   a single `Stream`. After the interaction was completed, the `Stream` will
///   be queried for its interests again.
/// - There exist a variety of iteration methods, which allow to iterate over
///   all or a subset of streams in each interest list.
pub struct StreamContainer<S> {
    /// Streams organized as a tree, for lookup by Stream ID
    stream_map: RBTree<StreamTreeAdapter<S>>,
    /// The number of streams which are tracked by the Container.
    /// This needs to be in-sync with Streams that get inserted into `stream_map`.
    nr_active_streams: usize,
    /// Additional interest lists in which Streams will be placed dynamically
    interest_lists: InterestLists<S>,
}

impl<S> core::fmt::Debug for StreamContainer<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        f.debug_struct("StreamContainer")
            .field("nr_active_streams", &self.nr_active_streams)
            .finish()
    }
}

macro_rules! iterate_uninterruptible {
    ($sel:ident, $list_name:tt, $link_name:ident, $controller:ident, $func:ident) => {
        for stream in $sel.interest_lists.$list_name.take() {
            debug_assert!(!stream.$link_name.is_linked());

            let interests = {
                let mut mut_stream = stream.inner.borrow_mut();
                $func(&mut *mut_stream);
                mut_stream.get_stream_interests()
            };

            $sel.interest_lists.update_interests(
                &stream,
                interests,
                StreamContainerIterationResult::Continue,
            );
        }

        if !$sel.interest_lists.done_streams.is_empty() {
            $sel.finalize_done_streams($controller);
        }
    };
}

macro_rules! iterate_interruptible {
    ($sel:ident, $list_name:tt, $link_name:ident, $controller:ident, $func:ident) => {
        let mut extracted_list = $sel.interest_lists.$list_name.take();
        let mut cursor = extracted_list.front_mut();

        while let Some(stream) = cursor.remove() {
            // Note that while we iterate over the intrusive lists here
            // `stream` is part of no list anymore, since it also got dropped
            // from list that is described by the `cursor`.
            debug_assert!(!stream.$link_name.is_linked());
            let mut mut_stream = stream.inner.borrow_mut();
            let result = $func(&mut *mut_stream);

            // Update the interests after the interaction
            let interests = mut_stream.get_stream_interests();
            $sel.interest_lists
                .update_interests(&stream, interests, result);

            match result {
                StreamContainerIterationResult::BreakAndInsertAtBack => {
                    $sel.interest_lists
                        .$list_name
                        .front_mut()
                        .splice_after(extracted_list);
                    break;
                }
                StreamContainerIterationResult::Continue => {}
            }
        }

        if !$sel.interest_lists.done_streams.is_empty() {
            $sel.finalize_done_streams($controller);
        }
    };
}

impl<S: StreamTrait> StreamContainer<S> {
    /// Creates a new `StreamContainer`
    pub fn new() -> Self {
        Self {
            stream_map: RBTree::new(StreamTreeAdapter::new()),
            nr_active_streams: 0,
            interest_lists: InterestLists::new(),
        }
    }

    /// Insert a new Stream into the container
    pub fn insert_stream(&mut self, stream: S) {
        // Even though it likely might have none, it seems like it
        // would be better to avoid future bugs
        let interests = stream.get_stream_interests();

        let new_stream = Rc::new(StreamNode::new(stream));

        self.interest_lists.update_interests(
            &new_stream,
            interests,
            StreamContainerIterationResult::Continue,
        );

        self.stream_map.insert(new_stream);
        self.nr_active_streams += 1;
    }

    /// Returns the amount of streams which are tracked by the `StreamContainer`
    pub fn nr_active_streams(&self) -> usize {
        self.nr_active_streams
    }

    /// Returns true if the container contains a Stream with the given ID
    pub fn contains(&self, stream_id: StreamId) -> bool {
        !self.stream_map.find(&stream_id).is_null()
    }

    /// Looks up the `Stream` with the given ID and executes the provided function
    /// on it.
    ///
    /// After the transaction with the `Stream` had been completed, the `Stream`
    /// will get queried for its new interests, and all lists will be updated
    /// according to those.
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    ///
    /// `Stream`s which signal finalization interest will be removed from the
    /// `StreamContainer`.
    pub fn with_stream<F, R>(
        &mut self,
        stream_id: StreamId,
        controller: &mut stream::Controller,
        func: F,
    ) -> Option<R>
    where
        F: FnOnce(&mut S) -> R,
    {
        let node_ptr: Rc<StreamNode<S>>;
        let result: R;
        let interests;

        // This block is required since we mutably borrow `self` inside the
        // block in order to obtain a Stream reference and to executing the
        // provided method.
        // We need to release the borrow in order to be able to update the
        // Streams interests after having executed the method.
        {
            let node = self.stream_map.find(&stream_id).get()?;

            // We have to obtain an `Rc<StreamNode>` in order to be able to
            // perform interest updates later on. However the intrusive tree
            // API only provides us a raw reference.
            // Safety: We know that all of our StreamNode's are stored in
            // `Rc` pointers.
            node_ptr = unsafe { stream_node_rc_from_ref(node) };

            let stream: &mut S = &mut node.inner.borrow_mut();
            result = func(stream);
            interests = stream.get_stream_interests();
        }

        // Update the interest lists after the interactions and then remove
        // all finalized streams
        if self.interest_lists.update_interests(
            &node_ptr,
            interests,
            StreamContainerIterationResult::Continue,
        ) {
            self.finalize_done_streams(controller);
        }

        Some(result)
    }

    /// Removes all Streams in the `done` state from the `StreamManager`.
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn finalize_done_streams(&mut self, controller: &mut stream::Controller) {
        for stream in self.interest_lists.done_streams.take() {
            // Remove the Stream from `stream_map`
            let mut cursor = self.stream_map.find_mut(&stream.inner.borrow().stream_id());
            let remove_result = cursor.remove();
            debug_assert!(remove_result.is_some());
            self.nr_active_streams -= 1;

            // And remove the Stream from all other interest lists it might be
            // part of.
            let stream_ptr = &*stream as *const StreamNode<S>;

            macro_rules! remove_stream_from_list {
                ($list_name:ident, $link_name:ident) => {
                    if stream.$link_name.is_linked() {
                        // Safety: We know that the Stream is part of the list,
                        // because it is linked, and we never place Streams in
                        // other lists when `finalize_done_streams` is called.
                        let mut cursor = unsafe {
                            self.interest_lists
                                .$list_name
                                .cursor_mut_from_ptr(stream_ptr)
                        };
                        let remove_result = cursor.remove();
                        debug_assert!(remove_result.is_some());
                    }
                };
            }

            remove_stream_from_list!(waiting_for_frame_delivery, waiting_for_frame_delivery_link);
            remove_stream_from_list!(waiting_for_transmission, waiting_for_transmission_link);
            remove_stream_from_list!(waiting_for_retransmission, waiting_for_retransmission_link);
            remove_stream_from_list!(
                waiting_for_connection_flow_control_credits,
                waiting_for_connection_flow_control_credits_link
            );
            remove_stream_from_list!(
                waiting_for_stream_flow_control_credits,
                waiting_for_stream_flow_control_credits_link
            );

            controller.on_close_stream(stream.inner.borrow().stream_id());
        }
    }

    /// Iterates over all `Stream`s which are waiting for frame delivery,
    /// and executes the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_frame_delivery_list<F>(
        &mut self,
        controller: &mut stream::Controller,
        mut func: F,
    ) where
        F: FnMut(&mut S),
    {
        iterate_uninterruptible!(
            self,
            waiting_for_frame_delivery,
            waiting_for_frame_delivery_link,
            controller,
            func
        );
    }

    /// Iterates over all `Stream`s which waiting for connection flow control
    /// credits, and executes the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_connection_flow_credits_list<F>(
        &mut self,
        controller: &mut stream::Controller,
        mut func: F,
    ) where
        F: FnMut(&mut S) -> StreamContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_connection_flow_control_credits,
            waiting_for_connection_flow_control_credits_link,
            controller,
            func
        );
    }

    /// Iterates over all `Stream`s which are waiting for stream flow control
    /// credits, and executes the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_stream_flow_credits_list<F>(
        &mut self,
        controller: &mut stream::Controller,
        mut func: F,
    ) where
        F: FnMut(&mut S) -> StreamContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_stream_flow_control_credits,
            waiting_for_stream_flow_control_credits_link,
            controller,
            func
        );
    }

    /// Iterates over all `Stream`s which are waiting for transmission,
    /// and executes the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_transmission_list<F>(&mut self, controller: &mut stream::Controller, mut func: F)
    where
        F: FnMut(&mut S) -> StreamContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_transmission,
            waiting_for_transmission_link,
            controller,
            func
        );
    }

    /// Iterates over all `Stream`s which are waiting for retransmission,
    /// and executes the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_retransmission_list<F>(
        &mut self,
        controller: &mut stream::Controller,
        mut func: F,
    ) where
        F: FnMut(&mut S) -> StreamContainerIterationResult,
    {
        iterate_interruptible!(
            self,
            waiting_for_retransmission,
            waiting_for_retransmission_link,
            controller,
            func
        );
    }

    /// Iterates over all `Stream`s which are part of this container, and executes
    /// the given function on each `Stream`
    ///
    /// The `stream::Controller` will be notified of streams that have been
    /// closed to allow for further streams to be opened.
    pub fn iterate_streams<F>(&mut self, controller: &mut stream::Controller, mut func: F)
    where
        F: FnMut(&mut S),
    {
        // Note: We can not use iterate_uninterruptible here, because that
        // iteration will extract nodes from the list but not automatically insert
        // all Nodes back into the main interest list. `stream_map` is not
        // populated by the interest maps.

        for stream in self.stream_map.iter() {
            debug_assert!(stream.tree_link.is_linked());

            let mut mut_stream = stream.inner.borrow_mut();
            func(&mut *mut_stream);
            let interests = mut_stream.get_stream_interests();

            // Update the interest lists here
            // Safety: The stream reference is obtained from the RBTree, which
            // stores it's nodes as `Rc`
            let stream_node_rc = unsafe { stream_node_rc_from_ref(stream) };
            self.interest_lists.update_interests(
                &stream_node_rc,
                interests,
                StreamContainerIterationResult::Continue,
            );
        }

        if !self.interest_lists.done_streams.is_empty() {
            // Cleanup all `done` streams after we finished interacting with all
            // of them.
            self.finalize_done_streams(controller);
        }
    }

    /// Returns whether or not streams have data to send
    pub fn has_pending_streams(&self) -> bool {
        !self.interest_lists.waiting_for_transmission.is_empty()
            || !self.interest_lists.waiting_for_retransmission.is_empty()
    }
}

impl<S: StreamTrait> timer::Provider for StreamContainer<S> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        // TODO denormalize this into a single value
        for stream in self
            .interest_lists
            .waiting_for_stream_flow_control_credits
            .iter()
        {
            stream.inner.borrow().timers(query)?;
        }
        Ok(())
    }
}

impl<S: StreamTrait> transmission::interest::Provider for StreamContainer<S> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if !self.interest_lists.waiting_for_retransmission.is_empty() {
            query.on_lost_data()?;
        } else if !self.interest_lists.waiting_for_transmission.is_empty() {
            query.on_new_data()?;
        }

        Ok(())
    }
}

/// Return values for iterations over a `Stream` list.
/// The value instructs the iterator whether iteration will be continued.
#[derive(Clone, Copy, Debug)]
pub enum StreamContainerIterationResult {
    /// Continue iteration over the list
    Continue,
    /// Aborts the iteration over a list and add the remaining items at the
    /// back of the list
    BreakAndInsertAtBack,
}
