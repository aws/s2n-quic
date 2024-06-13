// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    ptr,
    task::{RawWaker, RawWakerVTable, Waker},
};

#[cfg(feature = "alloc")]
mod contract;
#[cfg(feature = "alloc")]
pub use contract::*;

/// Creates a new `Waker` that does nothing when `wake` is called.
///
/// This is mostly useful for writing tests that need a [`core::task::Context`] to poll
/// some futures, but are not expecting those futures to wake the waker or
/// do not need to do anything specific if it happens.
///
/// Upstream Tracking issue: <https://github.com/rust-lang/rust/issues/98286>
#[inline]
pub fn noop() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        // Cloning just returns a new no-op raw waker
        |_| RAW,
        // `wake` does nothing
        |_| {},
        // `wake_by_ref` does nothing
        |_| {},
        // Dropping does nothing as we don't allocate anything
        |_| {},
    );
    const RAW: RawWaker = RawWaker::new(ptr::null(), &VTABLE);

    unsafe { Waker::from_raw(RAW) }
}
