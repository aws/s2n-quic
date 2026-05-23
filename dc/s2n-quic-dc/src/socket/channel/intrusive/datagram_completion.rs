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

use crate::{endpoint::id::LocalSenderId, flow::queue::AutoWake, intrusive};
use core::{
    mem::ManuallyDrop,
    sync::atomic::{AtomicU64, AtomicU8, Ordering},
    task::Poll,
};
use parking_lot::Mutex;
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

const SHOULD_TRANSMIT: u8 = 0b01;
const RECEIVER_ALIVE: u8 = 0b10;
const INITIAL_STATE: u8 = SHOULD_TRANSMIT | RECEIVER_ALIVE;
/// Sentinel meaning "not yet assigned"
const UNSET_SENDER_IDX: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubscriptionMode {
    #[default]
    All,
    FailuresOnly,
}

struct Inner<T> {
    queue: intrusive::Queue<T>,
    recv_waker: Option<core::task::Waker>,
}

struct Shared<T> {
    mode: SubscriptionMode,
    /// Bitpacked flags:
    /// - bit 0: should_transmit
    /// - bit 1: receiver_alive
    flags: AtomicU8,
    /// Index of the sender socket that transmitted the first FlowInit frame for this
    /// completion channel.  Stamped by the assembler when it picks up the FlowInit frame.
    /// `UNSET_SENDER_IDX` means the FlowInit has not yet been transmitted by any sender.
    init_sender_idx: AtomicU64,
    /// The `attempt_id` assigned to the FlowInit frame when it was first transmitted.
    /// Stamped alongside `init_sender_idx` by the assembler.  The writer includes this
    /// value in subsequent FlowInitReset frames so the server can mark the attempt as
    /// finalized in its dedup window and reject any late-arriving FlowInit duplicate.
    /// `UNSET_SENDER_IDX` means the FlowInit has not yet been transmitted.
    init_attempt_id: AtomicU64,
    inner: Mutex<Inner<T>>,
}

pub fn new<T>() -> Receiver<T> {
    new_with_mode(SubscriptionMode::All)
}

pub fn new_with_mode<T>(mode: SubscriptionMode) -> Receiver<T> {
    let shared = Arc::new(Shared {
        mode,
        flags: AtomicU8::new(INITIAL_STATE),
        init_sender_idx: AtomicU64::new(UNSET_SENDER_IDX),
        init_attempt_id: AtomicU64::new(UNSET_SENDER_IDX),
        inner: Mutex::new(Inner {
            queue: intrusive::Queue::new(),
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

    /// Record which sender-socket index transmitted the FlowInit frame for this channel.
    ///
    /// Called by the assembler the first time it picks up a FlowInit frame. The writer
    /// reads this back via [`Receiver::init_sender_idx`] to route FlowInitReset and
    /// FlowInitFin through the same sender socket.
    #[inline]
    pub fn set_init_sender_idx(&self, id: LocalSenderId) {
        // Only stamp on the first transmission (compare-exchange from UNSET).
        let _ = self.shared.init_sender_idx.compare_exchange(
            UNSET_SENDER_IDX,
            id.as_varint().as_u64(),
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    /// Returns the sender-socket index stamped for the FlowInit, if any.
    #[inline]
    pub fn init_sender_idx(&self) -> Option<LocalSenderId> {
        let v = self.shared.init_sender_idx.load(Ordering::Acquire);
        let v = VarInt::new(v).ok()?;
        Some(LocalSenderId::new(v))
    }

    /// Record the `attempt_id` assigned to the FlowInit frame by the assembler.
    ///
    /// Stamped alongside [`set_init_sender_idx`].  The writer includes this value in
    /// FlowInitReset frames so the server can mark the attempt as finalized in its
    /// dedup window.
    #[inline]
    pub fn set_init_attempt_id(&self, id: VarInt) {
        let _ = self.shared.init_attempt_id.compare_exchange(
            UNSET_SENDER_IDX,
            id.as_u64(),
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    /// Returns the `attempt_id` stamped for the FlowInit, if any.
    #[inline]
    pub fn init_attempt_id(&self) -> Option<VarInt> {
        match self.shared.init_attempt_id.load(Ordering::Acquire) {
            UNSET_SENDER_IDX => None,
            id => VarInt::new(id).ok(),
        }
    }

    /// Send a completion notification, returning the receiver waker rather than invoking it.
    ///
    /// If the receiver is no longer alive, the completion is silently dropped.
    pub fn send_entry(&self, entry: intrusive::Entry<T>) -> Result<AutoWake, intrusive::Entry<T>> {
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
        mut batch: intrusive::Queue<T>,
    ) -> Result<AutoWake, intrusive::Queue<T>> {
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

impl<T> super::super::UnboundedSender<intrusive::Entry<T>> for Sender<T> {
    fn send(&mut self, value: intrusive::Entry<T>) -> Result<(), intrusive::Entry<T>> {
        self.send_entry(value).map(drop)
    }
}

impl<T> super::super::Sender<intrusive::Entry<T>> for Sender<T> {
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive::Entry<T>>,
    ) -> Poll<Result<(), ()>> {
        let entry = unsafe { value.assume_init_read() };
        match self.send_entry(entry) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(_) => Poll::Ready(Err(())),
        }
    }
}

impl<T> super::super::UnboundedSender<intrusive::Queue<T>> for Sender<T> {
    fn send(&mut self, batch: intrusive::Queue<T>) -> Result<(), intrusive::Queue<T>> {
        self.send_batch(batch).map(drop)
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

    /// Returns the sender-socket index that transmitted the FlowInit frame, if known.
    ///
    /// Returns `None` if the FlowInit has not yet been picked up by any sender socket.
    #[inline]
    pub fn init_sender_idx(&self) -> Option<LocalSenderId> {
        let v = self.shared.init_sender_idx.load(Ordering::Acquire);
        let v = VarInt::new(v).ok()?;
        Some(LocalSenderId::new(v))
    }

    /// Returns the `attempt_id` assigned to the FlowInit by the assembler, if known.
    ///
    /// Returns `None` if the FlowInit has not yet been transmitted.
    #[inline]
    pub fn init_attempt_id(&self) -> Option<VarInt> {
        match self.shared.init_attempt_id.load(Ordering::Acquire) {
            UNSET_SENDER_IDX => None,
            id => VarInt::new(id).ok(),
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
    ) -> Poll<Option<intrusive::Queue<T>>> {
        let mut guard = self.shared.inner.lock();

        // Always register waker since we're draining everything
        // Use will_wake to avoid cloning if it's the same waker
        if guard
            .recv_waker
            .as_ref()
            .is_none_or(|w| !w.will_wake(cx.waker()))
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
        assert!(tx.send_entry(intrusive::Entry::new(())).is_ok());
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
