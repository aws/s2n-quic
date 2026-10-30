// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task;
use s2n_quic_core::{state::is, task::waker::noop};
use std::sync::Mutex;

/// An atomic waker that doesn't change often
#[derive(Debug)]
pub struct Waker {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    waker: task::Waker,
    status: Status,
}

impl Default for Waker {
    #[inline]
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                waker: noop(),
                status: Status::Sleeping,
            }),
        }
    }
}

impl Waker {
    #[inline]
    #[expect(
        clippy::unwrap_used,
        reason = "lock is only poisoned if another thread already panicked while holding it"
    )]
    pub fn update(&self, waker: &task::Waker) {
        self.state.lock().unwrap().waker = waker.clone();
    }

    #[inline]
    pub fn wake(&self) {
        self.on_event(Event::Notify);
    }

    #[inline]
    pub fn wake_forced(&self) {
        // wake the waker outside of the lock to avoid deadlocks
        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let waker = self.state.lock().unwrap().waker.clone();
        waker.wake();
    }

    /// Called when the worker wakes
    #[inline]
    pub fn on_worker_wake(&self) {
        self.on_event(Event::BeginPoll);
    }

    /// Called before the worker sleeps
    ///
    /// Schedules another poll if work arrived during this poll. Returns the previous [`Status`].
    #[inline]
    pub fn on_worker_sleep(&self) -> Status {
        self.on_event(Event::EndPoll)
    }

    #[inline]
    fn on_event(&self, event: Event) -> Status {
        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let mut state = self.state.lock().unwrap();
        let previous = state.status;
        let (status, should_wake) = previous.on_event(event);
        state.status = status;
        let waker = should_wake.then(|| state.waker.clone());
        drop(state);

        if let Some(waker) = waker {
            waker.wake();
        }

        previous
    }
}

#[derive(Clone, Copy)]
enum Event {
    Notify,
    BeginPoll,
    EndPoll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The worker is not polling.
    Sleeping,
    /// Work arrived during the current poll and requires another poll.
    PendingWork,
    /// The worker is polling without another notification pending.
    Working,
}

impl Status {
    #[inline]
    fn on_event(self, event: Event) -> (Self, bool) {
        use Event::*;
        use Status::*;

        match (event, self) {
            (Notify, Sleeping) => (Sleeping, true),
            (Notify, Working | PendingWork) => (PendingWork, false),
            (BeginPoll, _) => (Working, false),
            (EndPoll, PendingWork) => (Sleeping, true),
            (EndPoll, Sleeping | Working) => (Sleeping, false),
        }
    }

    is!(is_sleeping, Sleeping);
    is!(is_pending_work, PendingWork);
    is!(is_working, Working);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    };

    #[derive(Default)]
    struct Counter(AtomicUsize);

    impl std::task::Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn waker() -> (Waker, Arc<Counter>) {
        let counter = Arc::new(Counter::default());
        let waker = Waker::default();
        waker.update(&task::Waker::from(counter.clone()));
        (waker, counter)
    }

    #[test]
    fn wake_while_sleeping_schedules_poll() {
        let (waker, counter) = waker();
        waker.wake();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn wakes_during_poll_coalesce_and_are_consumed_by_next_poll() {
        let (waker, counter) = waker();
        waker.on_worker_wake();
        waker.wake();
        waker.wake();
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);

        assert_eq!(waker.on_worker_sleep(), Status::PendingWork);
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);

        waker.on_worker_wake();
        assert_eq!(waker.on_worker_sleep(), Status::Working);
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn notifications_on_either_side_of_sleep_schedule_poll() {
        for notify_before_sleep in [true, false] {
            let (waker, counter) = waker();
            waker.on_worker_wake();
            if notify_before_sleep {
                waker.wake();
            }
            waker.on_worker_sleep();
            if !notify_before_sleep {
                waker.wake();
            }
            assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn forced_wake_preserves_notification_during_poll() {
        let (waker, counter) = waker();
        waker.on_worker_wake();
        waker.wake();
        waker.wake_forced();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(waker.on_worker_sleep(), Status::PendingWork);
        assert_eq!(counter.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn task_wake_can_reenter_worker_waker() {
        struct Reentrant {
            worker: Weak<Waker>,
            counter: AtomicUsize,
        }

        impl std::task::Wake for Reentrant {
            fn wake(self: Arc<Self>) {
                let worker = self.worker.upgrade().unwrap();
                worker.update(&noop());
                self.counter.fetch_add(1, Ordering::Relaxed);
            }
        }

        let worker = Arc::new(Waker::default());
        let reentrant = Arc::new(Reentrant {
            worker: Arc::downgrade(&worker),
            counter: Default::default(),
        });
        let task_waker = task::Waker::from(reentrant.clone());

        worker.update(&task_waker);
        worker.wake();
        assert_eq!(reentrant.counter.load(Ordering::Relaxed), 1);

        worker.update(&task_waker);
        worker.on_worker_wake();
        worker.wake();
        worker.on_worker_sleep();
        assert_eq!(reentrant.counter.load(Ordering::Relaxed), 2);
    }
}
