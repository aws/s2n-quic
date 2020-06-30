// Silence the clippy warning that shows up when using the intrusive adapter
#![allow(clippy::unneeded_field_pattern)]

use alloc::rc::Rc;
use core::cell::RefCell;
use intrusive_collections::{intrusive_adapter, KeyAdapter, RBTree, RBTreeLink};
use s2n_quic_core::time::Timestamp;

/// The state of a single timer
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimerState {
    /// The timer is active
    Active,
    /// The timer is not active
    Inactive,
}

/// A timer node which is part of a timer tree
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimerNodeInner<T> {
    /// The timestamp when the timer expires
    pub expiration: Timestamp,
    /// The state of the timer
    pub state: TimerState,
    /// A token which is carried inside the timer.
    /// It can be used to identify the expired timer.
    pub timer_token: T,
}

impl<T> TimerNodeInner<T> {
    pub fn is_active(&self) -> bool {
        self.state == TimerState::Active
    }
}

/// A timer node which is part of a timer tree
//#[derive(Copy, Clone, PartialEq, Eq)]
#[derive(Debug)]
pub struct TimerNode<T> {
    /// The actual state of the timer
    pub inner: RefCell<TimerNodeInner<T>>,
    /// Allows the `TimerNode`s to be part of a timer tree
    pub tree_link: RBTreeLink,
}

impl<T> TimerNode<T> {
    // Creates a new `TimerNode` in `Active` state.
    pub fn new_active(timer_token: T, expiration: Timestamp) -> Self {
        Self {
            inner: RefCell::new(TimerNodeInner {
                expiration,
                state: TimerState::Active,
                timer_token,
            }),
            tree_link: RBTreeLink::new(),
        }
    }
}

// Intrusive red black tree adapter inserting TimerNodes into the timer tree
intrusive_adapter!(pub TimerTreeAdapter<T> = Rc<TimerNode<T>>: TimerNode<T> {
    tree_link: RBTreeLink
});

// This is required to build an intrusive `RBTree` of `TimerNode`s which
// utilizes `TimerId`s as a key. We want the timer tree to be sorted
// by Timestamps. Therefore the type of the key is `Timestamp`.
impl<'a, T> KeyAdapter<'a> for TimerTreeAdapter<T> {
    type Key = Timestamp;

    fn get_key(&self, x: &'a TimerNode<T>) -> Timestamp {
        x.inner.borrow().expiration
    }
}

/// Internal state of the timer
#[derive(Debug)]
pub struct TimerSharedState<T> {
    pub timers: RBTree<TimerTreeAdapter<T>>,
}

impl<T: Copy> TimerSharedState<T> {
    pub fn new() -> Self {
        Self {
            timers: RBTree::new(TimerTreeAdapter::new()),
        }
    }

    pub fn next_expiration(&self) -> Option<Timestamp> {
        let cursor = self.timers.front();
        cursor
            .get()
            .map(|timer_node| timer_node.inner.borrow().expiration)
    }

    pub fn pop_expired(&mut self, current_time: Timestamp) -> Option<T> {
        let mut front_cursor = self.timers.front_mut();
        let first = front_cursor.get();
        match first {
            Some(node) => {
                let mut timer_guard = node.inner.borrow_mut();
                // Check if the timer actually expired
                if timer_guard.expiration > current_time {
                    None
                } else {
                    let token = timer_guard.timer_token;
                    // Deactivate the timer
                    timer_guard.state = TimerState::Inactive;
                    drop(timer_guard);
                    front_cursor.remove();
                    Some(token)
                }
            }
            None => None,
        }
    }
}
