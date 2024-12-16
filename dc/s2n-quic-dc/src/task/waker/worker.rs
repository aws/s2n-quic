// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    sync::atomic::{AtomicU8, Ordering},
    task,
};
use crossbeam_epoch::{pin, Atomic};
use s2n_quic_core::{ensure, state::is};

/// An atomic waker that doesn't change often
#[derive(Debug)]
pub struct Waker {
    waker: Atomic<task::Waker>,
    has_woken: AtomicU8,
}

impl Default for Waker {
    #[inline]
    fn default() -> Self {
        Self {
            waker: Atomic::null(),
            has_woken: AtomicU8::new(Status::Sleeping.as_u8()),
        }
    }
}

impl Waker {
    #[inline]
    pub fn update(&self, waker: &task::Waker) {
        let pin = crossbeam_epoch::pin();

        let waker = crossbeam_epoch::Owned::new(waker.clone()).into_shared(&pin);
        let prev = self.waker.swap(waker, Ordering::AcqRel, &pin);

        ensure!(!prev.is_null());

        unsafe {
            pin.defer_unchecked(move || {
                drop(prev.try_into_owned());
            })
        }
    }

    #[inline]
    pub fn wake(&self) {
        let status = self.swap_status(Status::PendingWork, Ordering::Acquire);

        // we only need to `wake_by_ref` if the worker is sleeping
        ensure!(matches!(status, Status::Sleeping));

        self.wake_forced();
    }

    #[inline]
    pub fn wake_forced(&self) {
        let guard = crossbeam_epoch::pin();
        let waker = self.waker.load(Ordering::Acquire, &guard);
        let Some(waker) = (unsafe { waker.as_ref() }) else {
            return;
        };
        waker.wake_by_ref();
    }

    /// Called when the worker wakes
    #[inline]
    pub fn on_worker_wake(&self) {
        self.swap_status(Status::Working, Ordering::Release);
    }

    /// Called before the worker sleeps
    ///
    /// Returns the previous [`Status`]
    #[inline]
    pub fn on_worker_sleep(&self) -> Status {
        self.swap_status(Status::Sleeping, Ordering::Release)
    }

    #[inline]
    fn swap_status(&self, status: Status, ordering: Ordering) -> Status {
        Status::from_u8(self.has_woken.swap(status.as_u8(), ordering))
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

    #[inline]
    fn as_u8(self) -> u8 {
        match self {
            Self::Sleeping => 0,
            Self::PendingWork => 1,
            Self::Working => 2,
        }
    }

    #[inline]
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::PendingWork,
            2 => Self::Working,
            _ => Self::Sleeping,
        }
    }
}

impl Drop for Waker {
    #[inline]
    fn drop(&mut self) {
        let pin = pin();
        let waker = crossbeam_epoch::Shared::null();
        let prev = self.waker.swap(waker, Ordering::AcqRel, &pin);

        ensure!(!prev.is_null());

        unsafe {
            pin.defer_unchecked(move || {
                drop(prev.try_into_owned());
            })
        }
    }
}
