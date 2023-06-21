// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::primitive::{Arc, AtomicBool, Ordering};
use core::{
    pin::Pin,
    task::{Context, Poll, Waker},
};
use crossbeam_utils::CachePadded;

pub use crate::sync::primitive::AtomicWaker;

/// Creates a pair of attached atomic wakers
pub fn pair() -> (Handle, Handle) {
    let storage = Arc::pin(Storage::default());

    let a_ptr = &storage.a as *const _;
    let b_ptr = &storage.b as *const _;
    let is_open = &*storage.is_open as *const _;

    let a = Handle {
        local: a_ptr,
        remote: b_ptr,
        is_open,
        storage: storage.clone(),
    };

    let b = Handle {
        local: b_ptr,
        remote: a_ptr,
        is_open,
        storage: storage.clone(),
    };

    (a, b)
}

/// An attached atomic waker
#[derive(Debug)]
pub struct Handle {
    // store pointers so we don't have to go through the Arc pointer first
    local: *const AtomicWaker,
    remote: *const AtomicWaker,
    is_open: *const AtomicBool,
    #[allow(dead_code)]
    storage: Pin<Arc<Storage>>,
}

/// Safety: Pointers live as long as the storage
unsafe impl Send for Handle {}
/// Safety: Pointers live as long as the storage
unsafe impl Sync for Handle {}

impl Handle {
    /// Registers the local task for notifications from the other handle
    #[inline]
    pub fn register(&self, waker: &Waker) {
        unsafe { (*self.local).register(waker) }
    }

    /// Notifies the other handle that it should be woken up
    #[inline]
    pub fn wake(&self) {
        unsafe { (*self.remote).wake() }
    }

    /// Returns if the handle is open
    ///
    /// If `false` is returned, the other handle has been dropped and not interested in
    /// notifications anymore.
    #[inline]
    pub fn is_open(&self) -> bool {
        unsafe { (*self.is_open).load(Ordering::Acquire) }
    }

    /// Polls the handle until the peer handle has been closed
    #[inline]
    pub fn poll_close(&mut self, cx: &mut Context) -> Poll<()> {
        if !self.is_open() {
            return Poll::Ready(());
        }

        self.register(cx.waker());

        if !self.is_open() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug)]
struct Storage {
    a: AtomicWaker,
    b: AtomicWaker,
    is_open: CachePadded<AtomicBool>,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            a: Default::default(),
            b: Default::default(),
            is_open: CachePadded::new(AtomicBool::new(true)),
        }
    }
}

impl Drop for Handle {
    #[inline]
    fn drop(&mut self) {
        // set that we've closed our handle and notify the peer
        unsafe {
            (*self.is_open).store(false, Ordering::Release);
        }
        self.wake();
    }
}
