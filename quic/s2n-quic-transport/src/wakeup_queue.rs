// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A queue which allows to wake up a QUIC endpoint which is blocked on packet
//! reception or timers. This queue is used in case connections inside the endpoint
//! change their readiness state (e.g. they get ready to write).

use alloc::{collections::VecDeque, sync::Arc};
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Waker},
};
use std::{sync::Mutex, task::Wake};

/// The shared state of the [`WakeupQueue`].
#[derive(Debug)]
struct QueueState<T> {
    /// The IDs of connections which have been woken
    woken_connections: VecDeque<T>,
    /// The waker which should be used to wake up the connection
    waker: Option<Waker>,
    /// Whether a wakeup is already in progress
    wakeup_in_progress: bool,
}

impl<T: Copy + Send + Sync> QueueState<T> {
    fn new() -> Self {
        Self {
            woken_connections: VecDeque::new(),
            waker: None,
            wakeup_in_progress: false,
        }
    }

    fn queue_wakeup(&mut self, wakeup_handle_id: T) -> Option<Waker> {
        self.woken_connections.push_back(wakeup_handle_id);
        // If pushing another handle already notified the processing thread that it should dequeue
        // pending notifications there is no need to do this again.
        if self.wakeup_in_progress {
            return None;
        }

        self.wakeup_in_progress = true;
        self.waker.clone()
    }

    /// Polls for queued wakeup events.
    ///
    /// The method gets passed a queued which is used to store further wakeup events.
    /// It will returns a queue of occurred events.
    /// If no wakeup occurred, the method will store the passed [`Waker`] and notify it as soon as
    /// a wakeup occurred.
    fn poll_pending_wakeups(&mut self, swap_queue: &mut VecDeque<T>, context: &Context) {
        debug_assert!(swap_queue.is_empty());
        self.wakeup_in_progress = false;

        if self.woken_connections.is_empty() {
            // If no wakeup was pending, store or update the `Waker`
            match &self.waker {
                Some(w) => {
                    if !w.will_wake(context.waker()) {
                        self.waker = Some(context.waker().clone());
                    }
                }
                None => self.waker = Some(context.waker().clone()),
            }

            return;
        }

        core::mem::swap(&mut self.woken_connections, swap_queue);
    }

    #[cfg(any(feature = "testing", test))]
    fn test_inspect(&self) -> (usize, bool) {
        (self.woken_connections.len(), self.wakeup_in_progress)
    }
}

/// A queue which allows individual components to wakeups to a common blocked thread.
///
/// Multiple components can notify the thread to unblocked and to dequeue handles of components.
/// Each component is identified by a handle of type `T`.
///
/// A single thread is expected to deque the handles of blocked components and to inform those.
#[derive(Debug)]
pub struct WakeupQueue<T> {
    state: Arc<Mutex<QueueState<T>>>,
}

impl<T: Copy + Send + Sync> WakeupQueue<T> {
    /// Creates a new `WakeupQueue`.
    ///
    /// If a wakeup is triggered, the given [`Waker`] will be notified.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(QueueState::new())),
        }
    }

    /// Creates a new [`WakeupHandle`] which will wake up this [`WakeupQueue`] if
    /// [`WakeupHandle::wakeup`] is called.
    pub fn create_wakeup_handle(&self, wakeup_handle_id: T) -> WakeupHandle<T> {
        WakeupHandle::new(self.state.clone(), wakeup_handle_id)
    }

    /// Returns the list of component handles which need to get woken.
    /// Those component handles are retrieved inside a `VecDeque`. In order to avoid
    /// memory allocations, the caller is expected to pass in a new `VecDequeue` which will
    /// by utilized for further queueing. Thereby a double-buffering approach for wakeups is
    /// enabled.
    pub fn poll_pending_wakeups(&mut self, swap_queue: &mut VecDeque<T>, context: &Context) {
        let mut guard = self
            .state
            .lock()
            .expect("Locking can only fail if locks are poisoned");
        guard.poll_pending_wakeups(swap_queue, context)
    }

    #[cfg(any(feature = "testing", test))]
    fn test_state(&self) -> Arc<Mutex<QueueState<T>>> {
        self.state.clone()
    }
}

/// A handle which refers to a wakeup queue. The handles allows to notify the
/// queue that a wakeup is required, and that after the wakeup the owner of the handle
/// wants to be notified.
#[derive(Debug)]
pub struct WakeupHandle<T> {
    /// The queue this handle is referring to
    queue: Arc<Mutex<QueueState<T>>>,
    /// The internal ID of this wakeup handle. This can be used to distinguish which
    /// handle had woken up the [`WakeupQueue`].
    wakeup_handle_id: T,
    /// Whether a wakeup for this handle had already been queued since the last time
    /// the wakeup handler was called
    wakeup_queued: AtomicBool,
}

impl<T: Copy + Send + Sync> WakeupHandle<T> {
    /// Creates a new [`WakeupHandle`] which delegates wakeups to the given `queue`.
    fn new(queue: Arc<Mutex<QueueState<T>>>, wakeup_handle_id: T) -> Self {
        Self {
            queue,
            wakeup_handle_id,
            wakeup_queued: AtomicBool::new(false),
        }
    }

    /// Notifies the queue to wake up. If a `wakeup()` had been issued for the same
    /// [`WakeupHandle`] without having been handled yet, the new [`wakeup()`] request will be
    /// ignored, since the wakeup will already be pending.
    pub fn wakeup(&self) {
        // Check if a wakeup had been queued earlier
        if self.wakeup_queued.swap(true, Ordering::SeqCst) {
            return;
        }

        // Enqueue the wakeup request
        let maybe_waker = {
            let mut guard = self
                .queue
                .lock()
                .expect("Locking can only fail if locks are poisoned");
            guard.queue_wakeup(self.wakeup_handle_id)
        };

        // If the queue handling thread wasn't notified earlier by another thread,
        // notify it now.
        if let Some(waker) = maybe_waker {
            waker.wake();
        }
    }

    /// Notifies the `WakeupHandle` that a wakeup for this handle had been processed.
    ///
    /// Further calls to [`wakeup`] will be queued again.
    pub fn wakeup_handled(&self) {
        self.wakeup_queued.store(false, Ordering::SeqCst);
    }
}

impl<T: Copy + Send + Sync> Wake for WakeupHandle<T> {
    fn wake(self: Arc<Self>) {
        self.wakeup()
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakeup()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_test::task::new_count_waker;

    macro_rules! vec_deque {
        ($($value:expr),*) => {{
            #[allow(unused_mut)]
            let mut value = VecDeque::new();
            $(
                value.push_back($value);
            )*
            value
        }}
    }

    #[cfg(any(feature = "testing", test))]
    #[test]
    fn waker() {
        // check state len and if pending wakeup
        macro_rules! check_state {
            ($state:ident, $len:literal, $wakeup_in_progress:literal) => {{
                let state = $state
                    .lock()
                    .expect("Locking can only fail if locks are poisoned");
                assert_eq!(state.test_inspect(), ($len, $wakeup_in_progress));
            }};
        }

        // check queued wakeups and reset state (wakeup_handled)
        macro_rules! reset_state {
            ($state:ident, $len:literal, $handle1:ident, $handle2:ident) => {{
                let mut state = $state
                    .lock()
                    .expect("Locking can only fail if locks are poisoned");
                let (waker, _counter) = new_count_waker();
                let mut pending = VecDeque::new();
                state.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
                assert_eq!(pending.len(), $len);

                $handle1.wakeup_handled();
                $handle2.wakeup_handled();
            }};
        }

        let queue = WakeupQueue::new();
        let state = queue.test_state();
        check_state!(state, 0, false);

        let handle1 = Arc::new(queue.create_wakeup_handle(1u32));
        let handle2 = Arc::new(queue.create_wakeup_handle(2u32));
        let waker1 = Waker::from(handle1.clone());
        let waker2 = Waker::from(handle2.clone());

        // wake_by_ref
        waker1.wake_by_ref();
        check_state!(state, 1, true);
        // calling wake on same handle should not queue a wakeup
        waker1.wake_by_ref();
        check_state!(state, 1, true);
        waker2.wake_by_ref();
        check_state!(state, 2, true);

        reset_state!(state, 2, handle1, handle2);
        check_state!(state, 0, false);

        // clone and call wake
        waker1.wake_by_ref();
        check_state!(state, 1, true);
        waker2.wake_by_ref();
        check_state!(state, 2, true);

        // drop handle
        drop(waker1.clone());
        drop(waker2.clone());

        reset_state!(state, 2, handle1, handle2);
        check_state!(state, 0, false);

        // call wake on original handle
        waker1.wake();
        check_state!(state, 1, true);
        waker2.wake();
        check_state!(state, 2, true);
    }

    #[test]
    fn queue_wakeups() {
        let (waker, counter) = new_count_waker();
        let mut queue = WakeupQueue::new();
        let mut pending = VecDeque::new();

        let handle1 = queue.create_wakeup_handle(1u32);
        let handle2 = queue.create_wakeup_handle(2u32);
        assert_eq!(counter, 0);

        // Initially no wakeup should be signalled - but the Waker should be stored
        queue.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
        assert!(pending.is_empty());

        // After a wakeup the waker should be notified
        handle1.wakeup();
        assert_eq!(counter, 1);
        // A second wakeup on the same handle should not lead to another global wakeup
        handle1.wakeup();
        assert_eq!(counter, 1);

        // Even a second wakeup on the other handle should not lead to a global wakeup
        handle2.wakeup();
        assert_eq!(counter, 1);

        // The pending wakeups should be signaled
        queue.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
        assert_eq!(vec_deque![1u32, 2u32], pending);
        pending.clear();

        // In the next query no wakeups should be signaled
        queue.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
        assert!(pending.is_empty());

        // As long as wakeups are not handled, no new ones are enqueued
        handle2.wakeup();
        assert_eq!(counter, 1);
        queue.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
        assert!(pending.is_empty());

        // If wakeups are handled, wakeups are forwarded again
        handle1.wakeup_handled();
        handle2.wakeup_handled();

        handle2.wakeup();
        assert_eq!(counter, 2);
        queue.poll_pending_wakeups(&mut pending, &Context::from_waker(&waker));
        assert_eq!(vec_deque![2u32], pending);
        pending.clear();
    }
}
