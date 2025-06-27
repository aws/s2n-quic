// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task;
use s2n_quic_core::{ensure, state::is, task::waker::noop};
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
    pub fn update(&self, waker: &task::Waker) {
        self.state.lock().unwrap().waker = waker.clone();
    }

    #[inline]
    pub fn wake(&self) {
        let state = self.state.lock().unwrap();

        // we only need to `wake_by_ref` if the worker is sleeping
        ensure!(matches!(state.status, Status::Sleeping));

        // clone the waker out of the lock to avoid deadlocks
        let waker = state.waker.clone();
        drop(state);
        waker.wake();
    }

    #[inline]
    pub fn wake_forced(&self) {
        // clone the waker out of the lock to avoid deadlocks
        let waker = self.state.lock().unwrap().waker.clone();
        waker.wake();
    }

    /// Called when the worker wakes
    #[inline]
    pub fn on_worker_wake(&self) {
        self.swap_status(Status::Working);
    }

    /// Called before the worker sleeps
    ///
    /// Returns the previous [`Status`]
    #[inline]
    pub fn on_worker_sleep(&self) -> Status {
        self.swap_status(Status::Sleeping)
    }

    #[inline]
    fn swap_status(&self, status: Status) -> Status {
        let mut state = self.state.lock().unwrap();
        core::mem::replace(&mut state.status, status)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Sleeping,
    PendingWork,
    Working,
}

impl Status {
    is!(is_sleeping, Sleeping);
    is!(is_pending_work, PendingWork);
    is!(is_working, Working);
}
