// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::timer::shared_state::{TimerNode, TimerSharedState, TimerState};
use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::time::Timestamp;

/// A single timer instance, which is associated with a token of type `T`.
///
/// The timer can be activated via `set_expiration` and deactivated via `stop`.
/// If the timer expired, the [`TimerManager`] will yields its token.
#[derive(Debug)]
pub struct TimerEntry<T> {
    /// The shared state between timer and client
    pub(super) state: Rc<RefCell<TimerSharedState<T>>>,
    /// The timer node which is used by this timer instance
    pub(super) timer_node: Rc<TimerNode<T>>,
    /// The expiration which had been configured for the timer
    expiration: Option<Timestamp>,
}

impl<T> Drop for TimerEntry<T> {
    fn drop(&mut self) {
        self.stop()
    }
}

impl<T> TimerEntry<T> {
    /// Creates a new timer entry.
    ///
    /// This is only called internal to the timer module
    pub(super) fn new(
        state: Rc<RefCell<TimerSharedState<T>>>,
        timer_node: Rc<TimerNode<T>>,
    ) -> Self {
        Self {
            state,
            timer_node,
            expiration: None,
        }
    }

    /// Updates the Timer expiration if changed from previous value
    pub fn update(&mut self, desired_expiration: Option<Timestamp>) {
        if desired_expiration == self.expiration {
            return;
        }

        if let Some(expiration) = desired_expiration {
            self.set_expiration(expiration);
        } else {
            self.stop();
        }
    }

    /// Configures the Timer to expire at the given `Timestamp`.
    /// This will overwrite any previous timer setting.
    pub fn set_expiration(&mut self, expiration: Timestamp) {
        // Stop the timer if it is still active
        self.stop();

        self.expiration = Some(expiration);
        let mut timer_guard = self.timer_node.inner.borrow_mut();
        timer_guard.state = TimerState::Active;
        timer_guard.expiration = expiration;

        drop(timer_guard);

        // Insert the node into the timer list
        debug_assert!(!self.timer_node.tree_link.is_linked());

        let mut guard = self.state.borrow_mut();
        guard.timers.insert(self.timer_node.clone());
    }

    /// Stops the timer
    pub fn stop(&mut self) {
        self.expiration = None;

        let mut timer_guard = self.timer_node.inner.borrow_mut();
        // If the timer is not active, then it won't be in the timer list.
        if !timer_guard.is_active() {
            return;
        }

        timer_guard.state = TimerState::Inactive;
        drop(timer_guard);

        // Remove the timer from the list after it is no longer borrowed
        debug_assert!(self.timer_node.tree_link.is_linked());
        let mut guard = self.state.borrow_mut();

        // Safety: We know that the node is only ever part of this list
        let mut cursor = unsafe {
            guard
                .timers
                .cursor_mut_from_ptr(&*self.timer_node as *const TimerNode<T>)
        };
        cursor.remove();
    }
}
