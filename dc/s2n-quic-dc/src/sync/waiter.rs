// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::intrusive::{Adapter, Links, List};
use std::{cell::UnsafeCell, sync::Arc, task::Waker};

pub(crate) struct Waiter {
    pub(crate) links: Links,
    waker: UnsafeCell<Option<Waker>>,
}

impl Waiter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            links: Links::new(),
            waker: UnsafeCell::new(None),
        })
    }

    /// # Safety
    /// Must be called under the list's protecting Mutex.
    pub(crate) unsafe fn set_waker(&self, waker: Waker) {
        *self.waker.get() = Some(waker);
    }

    /// # Safety
    /// Must be called under the list's protecting Mutex.
    pub(crate) unsafe fn take_waker(&self) -> Option<Waker> {
        (*self.waker.get()).take()
    }
}

pub(crate) struct WaiterAdapter;

impl Adapter for WaiterAdapter {
    type Value = Waiter;
    type Target = Waiter;
    type Pointer = Arc<Waiter>;

    unsafe fn links(value: *mut Self::Value) -> *mut Links {
        &raw mut (*value).links
    }

    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
        value
    }

    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
        Arc::as_ptr(ptr)
    }

    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
        Arc::into_raw(ptr) as *mut Self::Value
    }

    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
        Arc::from_raw(ptr)
    }
}

pub(crate) type WaiterList = List<WaiterAdapter>;

// SAFETY: Waiter is Send+Sync because:
// - `Links` access is always under an external Mutex
// - `UnsafeCell<Option<Waker>>` access is always under the same Mutex
// - `Arc<Waiter>` is the pointer type (Send+Sync by Arc guarantees)
unsafe impl Send for Waiter {}
unsafe impl Sync for Waiter {}
