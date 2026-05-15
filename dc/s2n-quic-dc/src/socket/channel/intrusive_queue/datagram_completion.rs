// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Specialized intrusive queue channel for datagram completions.
//!
//! This channel has two independent state bits:
//! - `should_transmit`: Should workers transmit datagrams attached to this channel?
//! - `receiver_alive`: Is receiver around for completion notifications?
//!
//! Lifecycle:
//! - Active: Both bits set - transmit and notify
//! - Graceful shutdown: Receiver drops normally - transmit but silently drop completions
//! - Panic/Cancel: Receiver panics or sender.cancel() - cancel all pending transmissions

use crate::{flow::queue::AutoWake, intrusive_queue};
use core::{
    mem::ManuallyDrop,
    sync::atomic::{AtomicU8, Ordering},
    task::Poll,
};
use parking_lot::Mutex;
use std::sync::Arc;

const SHOULD_TRANSMIT: u8 = 0b01;
const RECEIVER_ALIVE: u8 = 0b10;
const INITIAL_STATE: u8 = SHOULD_TRANSMIT | RECEIVER_ALIVE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubscriptionMode {
    #[default]
    All,
    FailuresOnly,
}

struct Inner<T> {
    queue: intrusive_queue::Queue<T>,
    recv_waker: Option<core::task::Waker>,
}

struct Shared<T> {
    mode: SubscriptionMode,
    /// Bitpacked flags:
    /// - bit 0: should_transmit
    /// - bit 1: receiver_alive
    flags: AtomicU8,
    inner: Mutex<Inner<T>>,
}

pub fn new<T>() -> Receiver<T> {
    new_with_mode(SubscriptionMode::All)
}

pub fn new_with_mode<T>(mode: SubscriptionMode) -> Receiver<T> {
    let shared = Arc::new(Shared {
        mode,
        flags: AtomicU8::new(INITIAL_STATE),
        inner: Mutex::new(Inner {
            queue: intrusive_queue::Queue::new(),
            recv_waker: None,
        }),
    });
    Receiver {
        shared: ManuallyDrop::new(shared),
    }
}

pub struct Sender<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> core::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Sender")
            .field("should_transmit", &self.should_transmit())
            .finish()
    }
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
        // Check if this is the last sender (count will be 2: this sender + receiver)
        let is_last_sender = Arc::strong_count(&self.shared) == 2;

        // Wake the receiver only if this is the last sender
        if is_last_sender {
            let mut guard = self.shared.inner.lock();
            let waker = core::mem::take(&mut guard.recv_waker);
            drop(guard);

            if let Some(waker) = waker {
                waker.wake();
            }
        }

        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }
    }
}

impl<T> Sender<T> {
    /// Returns true if workers should transmit datagrams attached to this channel.
    #[inline]
    pub fn should_transmit(&self) -> bool {
        self.shared.flags.load(Ordering::Acquire) & SHOULD_TRANSMIT != 0
    }

    /// Returns a pointer address for this sender (for equality comparisons).
    ///
    /// This can be used to group datagrams by their completion channel.
    #[inline]
    pub fn queue_id(&self) -> usize {
        Arc::as_ptr(&self.shared) as usize
    }

    #[inline]
    pub fn subscription_mode(&self) -> SubscriptionMode {
        self.shared.mode
    }

    /// Send a completion notification, returning the receiver waker rather than invoking it.
    ///
    /// If the receiver is no longer alive, the completion is silently dropped.
    pub fn send_entry(
        &self,
        entry: intrusive_queue::Entry<T>,
    ) -> Result<AutoWake, intrusive_queue::Entry<T>> {
        let flags = self.shared.flags.load(Ordering::Acquire);

        // If receiver is gone, silently drop the completion
        if flags & RECEIVER_ALIVE == 0 {
            return Ok(AutoWake::default());
        }

        let mut guard = self.shared.inner.lock();
        guard.queue.push_back(entry);
        let waker = AutoWake::new(guard.recv_waker.take());
        Ok(waker)
    }

    /// Send a batch of completion notifications, returning the receiver waker if one was
    /// registered rather than invoking it inline.
    pub fn send_batch(
        &self,
        mut batch: intrusive_queue::Queue<T>,
    ) -> Result<AutoWake, intrusive_queue::Queue<T>> {
        if batch.is_empty() {
            return Ok(AutoWake::default());
        }

        let flags = self.shared.flags.load(Ordering::Acquire);

        // If receiver is gone, silently drop all completions
        if flags & RECEIVER_ALIVE == 0 {
            return Ok(AutoWake::default());
        }

        let mut guard = self.shared.inner.lock();
        guard.queue.append(&mut batch);
        let waker = AutoWake::new(guard.recv_waker.take());
        Ok(waker)
    }
}

impl<T> super::super::UnboundedSender<intrusive_queue::Entry<T>> for Sender<T> {
    fn send(&mut self, value: intrusive_queue::Entry<T>) -> Result<(), intrusive_queue::Entry<T>> {
        self.send_entry(value).map(drop)
    }
}

impl<T> super::super::Sender<intrusive_queue::Entry<T>> for Sender<T> {
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive_queue::Entry<T>>,
    ) -> Poll<Result<(), ()>> {
        let entry = unsafe { value.assume_init_read() };
        match self.send_entry(entry) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(_) => Poll::Ready(Err(())),
        }
    }
}

impl<T> super::super::UnboundedSender<intrusive_queue::Queue<T>> for Sender<T> {
    fn send(&mut self, batch: intrusive_queue::Queue<T>) -> Result<(), intrusive_queue::Queue<T>> {
        self.send_batch(batch).map(drop)
    }
}

impl<T> super::super::Sender<intrusive_queue::Queue<T>> for Sender<T> {
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive_queue::Queue<T>>,
    ) -> Poll<Result<(), ()>> {
        let batch = unsafe { value.assume_init_read() };

        if batch.is_empty() {
            return Poll::Ready(Ok(()));
        }

        match self.send_batch(batch) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(returned_batch) => {
                value.write(returned_batch);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<T> {
    shared: ManuallyDrop<Arc<Shared<T>>>,
}

impl<T> Receiver<T> {
    /// Creates a new sender for this receiver.
    ///
    /// This allows applications to create senders on-demand when they need to
    /// send datagrams, without having to store the sender permanently.
    #[inline]
    pub fn sender(&self) -> Sender<T> {
        Sender {
            shared: ManuallyDrop::new(Arc::clone(&self.shared)),
        }
    }

    /// Cancel all pending transmissions.
    ///
    /// Workers will drop any pending datagrams attached to this channel.
    #[inline]
    pub fn cancel(&self) {
        self.shared
            .flags
            .fetch_and(!SHOULD_TRANSMIT, Ordering::Release);
    }

    /// Swap out the entire queue in one operation, always registering the waker.
    ///
    /// This unconventionally registers the waker even when returning data, but it's
    /// correct because we're atomically draining the entire queue. Since the queue is
    /// guaranteed empty after this call, we know the waker needs to be registered for
    /// future wakeups. This avoids the need to loop calling poll_recv.
    pub fn poll_swap(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<intrusive_queue::Queue<T>>> {
        let mut guard = self.shared.inner.lock();

        // Always register waker since we're draining everything
        // Use will_wake to avoid cloning if it's the same waker
        if guard
            .recv_waker
            .as_ref()
            .map_or(true, |w| !w.will_wake(cx.waker()))
        {
            guard.recv_waker = Some(cx.waker().clone());
        }

        if !guard.queue.is_empty() {
            // Swap out the entire queue
            let batch = core::mem::take(&mut guard.queue);
            Poll::Ready(Some(batch))
        } else {
            // Queue is empty
            Poll::Pending
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Panic case - cancel all transmissions
            self.shared.flags.store(0, Ordering::Release);
        } else {
            // Graceful drop - just stop receiving completions
            self.shared
                .flags
                .fetch_and(!RECEIVER_ALIVE, Ordering::Release);
        }

        unsafe {
            ManuallyDrop::drop(&mut self.shared);
        }
    }
}

impl<T> super::super::Receiver<intrusive_queue::Entry<T>> for Receiver<T> {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive_queue::Entry<T>>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut guard = self.shared.inner.lock();

        if let Some(entry) = guard.queue.pop_front() {
            budget.consume();
            return Poll::Ready(Some(entry));
        }

        // Queue is empty and senders still alive - register waker
        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

impl<T> super::super::Receiver<intrusive_queue::Queue<T>> for Receiver<T> {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive_queue::Queue<T>>> {
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

        // Queue is empty and senders still alive - register waker
        guard.recv_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let rx = new::<u32>();
        let tx = rx.sender();
        assert!(tx.should_transmit());
    }

    #[test]
    fn cancel() {
        let rx = new::<u32>();
        let tx = rx.sender();
        rx.cancel();
        assert!(!tx.should_transmit());
    }

    #[test]
    fn graceful_receiver_drop() {
        let rx = new::<()>();
        let tx = rx.sender();
        assert!(tx.should_transmit());

        drop(rx); // Graceful drop

        // Should still transmit, but completions will be silently dropped
        assert!(tx.should_transmit());

        // Sending should succeed (silently drops)
        assert!(tx.send_entry(intrusive_queue::Entry::new(())).is_ok());
    }

    #[test]
    fn sender_drop_no_cancel() {
        let rx = new::<()>();
        let tx = rx.sender();
        let tx2 = tx.clone();

        drop(tx);

        // Sender drop doesn't cancel transmissions
        assert!(tx2.should_transmit());
    }

    #[test]
    fn sender_subscription_mode() {
        let rx = new_with_mode::<()>(SubscriptionMode::FailuresOnly);
        let tx = rx.sender();
        assert_eq!(tx.subscription_mode(), SubscriptionMode::FailuresOnly);

        let rx = new::<()>();
        let tx = rx.sender();
        assert_eq!(tx.subscription_mode(), SubscriptionMode::All);
    }
}
