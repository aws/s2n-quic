// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Synchronization primitives, swappable for loom-instrumented versions under test.
//!
//! Everything in the crate that needs an atomic, an `Arc`, a `Mutex`, or an `UnsafeCell` for
//! concurrency-sensitive code should import it from here rather than `std`/`core`/`parking_lot`
//! directly. Under `cfg(all(feature = "loom", test))` these resolve to loom's instrumented types so
//! the loom model checker can explore interleavings; otherwise they resolve to the production types
//! (`parking_lot::Mutex` for uncontended speed, std atomics/`Arc`).
//!
//! Because this is a plain module re-export (not a global `--cfg loom`), the swap is crate-scoped
//! and never leaks into dependencies.
//!
//! `Mutex` deliberately exposes the [`lock`] free function rather than a `.lock()` method so callers
//! are identical across the parking_lot (infallible) and loom (`Result`) APIs.

pub mod free_list;
pub(crate) mod waiter;
pub mod wake;

pub use wake::AutoWake;

#[cfg(all(feature = "loom", test))]
#[allow(unused_imports)]
mod imp {
    pub use loom::sync::{
        atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, RwLock,
    };

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap()
    }
}

#[cfg(not(all(feature = "loom", test)))]
#[allow(unused_imports)]
mod imp {
    pub use core::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
    pub use parking_lot::{Mutex, MutexGuard};
    pub use std::sync::{Arc, RwLock};

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock()
    }
}

#[allow(unused_imports)]
pub(crate) use imp::*;
