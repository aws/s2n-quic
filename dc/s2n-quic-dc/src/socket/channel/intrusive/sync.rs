// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Send-safe intrusive queue channel for normal async runtimes.
//!
//! The sender has no backpressure - it can always push entries to the queue.
//! The receiver drains the queue until empty, using wakers for notification.
use crate::{intrusive, tracing::*};
use core::{
    mem::ManuallyDrop,
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
};
use parking_lot::Mutex;
use std::sync::Arc;

struct Inner<T> {
    queue: intrusive::Queue<T>,
    recv_waker: Option<core::task::Waker>,
}

struct Shared<T> {
    /// True when the receiver is alive
    is_open: AtomicBool,
    inner: Mutex<Inner<T>>,
}

pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        is_open: AtomicBool::new(true),
        inner: Mutex::new(Inner {
            queue: intrusive::Queue::new(),
            recv_waker: None,
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

// ── Adapter-generic channel ───────────────────────────────────────────────

struct AdapterInner<A: intrusive::Adapter> {
    queue: intrusive::List<A>,
    recv_waker: Option<core::task::Waker>,
}

pub struct AdapterShared<A: intrusive::Adapter> {
    is_open: AtomicBool,
    inner: Mutex<AdapterInner<A>>,
}

impl<A: intrusive::Adapter> AdapterShared<A>
where
    A::Pointer: Send,
{
    pub(crate) fn push(&self, value: A::Pointer) -> Result<(), A::Pointer> {
        let mut guard = self.inner.lock();
        if !self.is_open.load(Ordering::Acquire) {
            return Err(value);
        }
        guard.queue.push_back(value);
        if let Some(waker) = guard.recv_waker.take() {
            drop(guard);
            waker.wake();
        }
        Ok(())
    }
}

pub fn new_with_adapter<A: intrusive::Adapter>() -> (AdapterSender<A>, AdapterReceiver<A>)
where
    A::Pointer: Send,
{
    let shared = Arc::new(AdapterShared {
        is_open: AtomicBool::new(true),
        inner: Mutex::new(AdapterInner {
            queue: intrusive::List::new(),
            recv_waker: None,
        }),
    });
    (
        AdapterSender {
            shared: ManuallyDrop::new(shared.clone()),
        },
        AdapterReceiver {
            shared: ManuallyDrop::new(shared),
        },
    )
}

pub struct AdapterSender<A: intrusive::Adapter> {
    shared: ManuallyDrop<Arc<AdapterShared<A>>>,
}

impl<A: intrusive::Adapter> AdapterSender<A>
where
    A::Pointer: Send,
{
    pub fn downgrade(&self) -> std::sync::Weak<AdapterShared<A>> {
        Arc::downgrade(&self.shared)
    }
}

impl<A: intrusive::Adapter> Clone for AdapterSender<A> {
    fn clone(&self) -> Self {
        Self {
            shared: ManuallyDrop::new(Arc::clone(&self.shared)),
        }
    }
}

impl<A: intrusive::Adapter> Drop for AdapterSender<A> {
    fn drop(&mut self) {
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

pub struct AdapterReceiver<A: intrusive::Adapter> {
    shared: ManuallyDrop<Arc<AdapterShared<A>>>,
}

impl<A: intrusive::Adapter> Drop for AdapterReceiver<A> {
    fn drop(&mut self) {
        self.shared.is_open.store(false, Ordering::Release);
        let mut guard = self.shared.inner.lock();
        let waker = core::mem::take(&mut guard.recv_waker);
        drop(guard);
        if let Some(waker) = waker {
            waker.wake();
        }
        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }
    }
}

impl<A: intrusive::Adapter> super::super::Receiver<intrusive::List<A>> for AdapterReceiver<A>
where
    A::Pointer: Send,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive::List<A>>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut guard = self.shared.inner.lock();

        if !guard.queue.is_empty() {
            let batch = core::mem::take(&mut guard.queue);
            budget.consume();
            return Poll::Ready(Some(batch));
        }

        if Arc::strong_count(&self.shared) <= 1 {
            return Poll::Ready(None);
        }

        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

// ── Original Entry-based channel ──────────────────────────────────────────

pub struct Sender<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: ManuallyDrop::new(Arc::clone(&self.shared)),
        }
    }
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

impl<T> super::super::UnboundedSender<intrusive::Entry<T>> for Sender<T> {
    fn send(&mut self, value: intrusive::Entry<T>) -> Result<(), intrusive::Entry<T>> {
        if !self.shared.is_open.load(Ordering::Acquire) {
            return Err(value);
        }

        let mut guard = self.shared.inner.lock();
        guard.queue.push_back(value);

        // Wake the receiver if it's waiting
        if let Some(waker) = guard.recv_waker.take() {
            drop(guard);
            waker.wake();
        }

        Ok(())
    }
}

impl<T> super::super::Sender<intrusive::Entry<T>> for Sender<T> {
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive::Entry<T>>,
    ) -> Poll<Result<(), ()>> {
        if !self.shared.is_open.load(Ordering::Acquire) {
            return Poll::Ready(Err(()));
        }

        let mut guard = self.shared.inner.lock();
        let entry = unsafe { value.assume_init_read() };
        guard.queue.push_back(entry);

        // Wake the receiver if it's waiting
        if let Some(waker) = guard.recv_waker.take() {
            drop(guard);
            waker.wake();
        }

        Poll::Ready(Ok(()))
    }
}

impl<T> Sender<T> {
    /// Send a single entry to the queue.
    ///
    /// This method takes `&self` so it can be called without cloning the sender.
    pub fn send_entry(&self, entry: intrusive::Entry<T>) -> Result<(), intrusive::Entry<T>> {
        if !self.shared.is_open.load(Ordering::Acquire) {
            return Err(entry);
        }

        let mut guard = self.shared.inner.lock();
        guard.queue.push_back(entry);

        // Wake the receiver if it's waiting
        if let Some(waker) = guard.recv_waker.take() {
            drop(guard);
            waker.wake();
        }

        Ok(())
    }

    /// Send a batch of entries by appending them to the queue.
    ///
    /// This is more efficient than sending entries one at a time as it only
    /// acquires the lock once and wakes the receiver once.
    ///
    /// This method takes `&self` so it can be called without cloning the sender.
    pub fn send_batch(&self, mut batch: intrusive::Queue<T>) -> Result<(), intrusive::Queue<T>> {
        if batch.is_empty() {
            return Ok(());
        }

        if !self.shared.is_open.load(Ordering::Acquire) {
            return Err(batch);
        }

        let mut guard = self.shared.inner.lock();
        guard.queue.append(&mut batch);

        // Wake the receiver if it's waiting
        if let Some(waker) = guard.recv_waker.take() {
            drop(guard);
            waker.wake();
        }

        Ok(())
    }
}

impl<T> super::super::UnboundedSender<intrusive::Queue<T>> for Sender<T> {
    fn send(&mut self, batch: intrusive::Queue<T>) -> Result<(), intrusive::Queue<T>> {
        self.send_batch(batch)
    }
}

impl<T> super::super::Sender<intrusive::Queue<T>> for Sender<T> {
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive::Queue<T>>,
    ) -> Poll<Result<(), ()>> {
        let batch = unsafe { value.assume_init_read() };

        if batch.is_empty() {
            return Poll::Ready(Ok(()));
        }

        match self.send_batch(batch) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(returned_batch) => {
                // Put the batch back and signal closed
                value.write(returned_batch);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // Mark channel as closed
        self.shared.is_open.store(false, Ordering::Release);

        // Wake any waiting senders (though senders don't currently wait, but future-proof)
        let mut guard = self.shared.inner.lock();
        let waker = core::mem::take(&mut guard.recv_waker);
        drop(guard);

        if let Some(waker) = waker {
            waker.wake();
        }

        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }
    }
}

impl<T> super::super::Receiver<intrusive::Entry<T>> for Receiver<T> {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive::Entry<T>>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut guard = self.shared.inner.lock();

        if let Some(entry) = guard.queue.pop_front() {
            budget.consume();
            return Poll::Ready(Some(entry));
        }

        // Check if all senders are gone (strong_count <= 1 means only receiver left)
        if Arc::strong_count(&self.shared) <= 1 {
            error!(
                strong_count = Arc::strong_count(&self.shared),
                "sync channel closed: all senders dropped"
            );
            return Poll::Ready(None);
        }

        // Queue is empty and senders still alive - register waker
        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

impl<T> super::super::Receiver<intrusive::Queue<T>> for Receiver<T> {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive::Queue<T>>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut guard = self.shared.inner.lock();

        if !guard.queue.is_empty() {
            // Drain all available entries into a batch
            let batch = core::mem::take(&mut guard.queue);
            budget.consume();
            return Poll::Ready(Some(batch));
        }

        // Check if all senders are gone (strong_count <= 1 means only receiver left)
        if Arc::strong_count(&self.shared) <= 1 {
            error!(
                strong_count = Arc::strong_count(&self.shared),
                "sync channel closed: all senders dropped"
            );
            return Poll::Ready(None);
        }

        // Queue is empty and senders still alive - register waker
        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}
