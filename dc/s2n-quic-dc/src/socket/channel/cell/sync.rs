// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{mem::ManuallyDrop, task::Poll};
use parking_lot::Mutex;
use std::sync::Arc;

struct Inner<T> {
    value: Option<T>,
    recv_waker: Option<core::task::Waker>,
    send_waker: Option<core::task::Waker>,
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
}

impl<T> Shared<T> {
    /// Returns `true` if both sender and receiver are still alive.
    #[inline]
    fn is_alive(self: &Arc<Self>) -> bool {
        Arc::strong_count(self) == 2
    }
}

pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            value: None,
            recv_waker: None,
            send_waker: None,
        }),
    });
    (
        Sender {
            shared: ManuallyDrop::new(shared.clone()),
        },
        Receiver {
            shared: ManuallyDrop::new(shared),
        },
    )
}

pub struct Sender<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Wake the receiver so it can observe the closed state
        let mut guard = self.shared.inner.lock();
        let waker = core::mem::take(&mut guard.recv_waker);
        drop(guard);
        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> super::super::Sender<T> for Sender<T> {
    fn poll_send(
        &mut self,
        cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<T>,
    ) -> Poll<Result<(), ()>> {
        if !self.shared.is_alive() {
            return Poll::Ready(Err(()));
        }

        let mut guard = self.shared.inner.lock();

        if guard.value.is_none() {
            guard.value = Some(unsafe { value.assume_init_read() });
            if let Some(waker) = guard.recv_waker.take() {
                drop(guard);
                waker.wake();
            }
            return Poll::Ready(Ok(()));
        }

        // Slot full — register waker for when receiver drains
        guard.send_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

pub struct Receiver<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // Wake the receiver so it can observe the closed state
        let mut guard = self.shared.inner.lock();
        let waker = core::mem::take(&mut guard.send_waker);
        drop(guard);
        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> super::super::Receiver<T> for Receiver<T> {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<T>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut guard = self.shared.inner.lock();
        if let Some(value) = guard.value.take() {
            budget.consume();
            if let Some(waker) = guard.send_waker.take() {
                drop(guard);
                waker.wake();
            }
            return Poll::Ready(Some(value));
        }
        if !self.shared.is_alive() {
            return Poll::Ready(None);
        }
        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}
