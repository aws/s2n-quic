// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(any(feature = "loom", test), allow(dead_code))]

//! A persistent task waker for the credit distributor.
//!
//! Production runtimes we target (tokio, bach, busy-poll) all hand the same `Waker` to a task
//! across polls — the waker is task-pinned. Under that assumption the storage can be a
//! [`std::sync::OnceLock`]: `register` clones the waker on the first poll and never writes again,
//! and `wake` is a lock-free atomic load + `wake_by_ref`. We `debug_assert!(will_wake)` on
//! subsequent registrations so a runtime that violates the assumption is loud in tests.
//!
//! Under `cfg(all(feature = "loom", test))` we use a `RwLock`-backed implementation so the
//! register/wake race is modeled. Loom's executor can hand a different waker on each poll, which
//! would trip the debug-assert; the loom impl takes the small extra cost of refreshing on change so
//! the model can explore those interleavings cleanly.

mod once_lock {
    use std::{sync::OnceLock, task::Waker};

    pub struct TaskWaker(OnceLock<Waker>);

    impl TaskWaker {
        #[inline]
        pub fn new() -> Self {
            Self(OnceLock::new())
        }

        /// Register the distributor's waker. Called only by the single distributor; the first call
        /// stores, later calls are a no-op (the runtime hands the same waker each poll).
        #[inline]
        pub fn register(&self, waker: &Waker) {
            for noop in [
                &s2n_quic_core::task::waker::noop(),
                core::task::Waker::noop(),
            ] {
                // no point in registering the noop waker
                if waker.data() == noop.data() && waker.vtable() == noop.vtable() {
                    return;
                }
            }

            let prev = self.0.get_or_init(|| waker.clone());
            debug_assert!(
                prev.will_wake(waker),
                "distributor waker changed across polls; this runtime is not supported by \
                 the OnceLock TaskWaker — switch to the RwLock impl or relax to refresh"
            );
        }

        /// Wake the distributor. Lock-free atomic load; never clears the stored waker.
        #[inline]
        pub fn wake(&self) {
            if let Some(waker) = self.0.get() {
                waker.wake_by_ref();
            }
        }
    }
}

#[cfg(any(feature = "loom", test))]
mod rw_lock {
    use crate::sync::RwLock;
    use std::task::Waker;

    pub struct TaskWaker(RwLock<Option<Waker>>);

    impl TaskWaker {
        pub fn new() -> Self {
            Self(RwLock::new(None))
        }

        /// Loom-only: refresh on change. Loom's `block_on` may produce a different waker each poll,
        /// so this impl tolerates it instead of asserting.
        pub fn register(&self, waker: &Waker) {
            if let Some(existing) = self.0.read().unwrap().as_ref() {
                if existing.will_wake(waker) {
                    return;
                }
            }
            *self.0.write().unwrap() = Some(waker.clone());
        }

        pub fn wake(&self) {
            if let Some(waker) = self.0.read().unwrap().as_ref() {
                waker.wake_by_ref();
            }
        }
    }
}

#[cfg(not(all(feature = "loom", test)))]
pub use once_lock::*;
#[cfg(all(feature = "loom", test))]
pub use rw_lock::*;
