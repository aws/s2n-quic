// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Acceptor channel with deferred waking and pick-two load balancing.
//!
//! Senders push items into receiver slots without performing wake syscalls inline.
//! Instead, `send` returns an `Option<Waker>` that the caller forwards to a waker
//! thread. Receivers clone to scale out, each getting a dedicated bounded queue.
//! When a receiver's queue is full, an item is evicted per the configured policy.

use crate::xorshift::Rng;
use core::task::{Poll, Waker};
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Eviction policy when a receiver's queue is at capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eviction {
    /// Remove the oldest item (front of queue) to make room. Standard FIFO behavior.
    Front,
    /// Remove the newest item (back of queue) to make room. LIFO-style pruning.
    Back,
}

/// Configuration for the channel.
#[derive(Debug, Clone)]
pub struct Config {
    pub capacity: usize,
    pub eviction: Eviction,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capacity: 1024,
            eviction: Eviction::Front,
        }
    }
}

impl From<usize> for Config {
    fn from(capacity: usize) -> Self {
        Self {
            capacity,
            ..Default::default()
        }
    }
}

/// Create a channel pair. Both sender and receiver are cloneable.
///
/// Cloning the receiver creates a new independent slot that receives a share
/// of future sends via pick-two load balancing.
pub fn new<T>(config: Config) -> (Sender<T>, Receiver<T>) {
    assert!(config.capacity > 0);

    let shared = Arc::new(Shared {
        slots: Mutex::new(Vec::new()),
        epoch: AtomicUsize::new(0),
        sender_count: AtomicUsize::new(1),
        receiver_count: AtomicUsize::new(0),
        config,
    });

    let sender = Sender {
        shared: shared.clone(),
        cached_slots: Vec::new(),
        cached_epoch: 0,
        rng: Rng::new(),
    };

    let receiver = Receiver::new_unregistered(&shared);

    (sender, receiver)
}

// ── Shared state ────────────────────────────────────────────────────────────

struct Shared<T> {
    slots: Mutex<Vec<Arc<Slot<T>>>>,
    epoch: AtomicUsize,
    sender_count: AtomicUsize,
    receiver_count: AtomicUsize,
    config: Config,
}

// ── Slot ────────────────────────────────────────────────────────────────────

struct Slot<T> {
    inner: Mutex<SlotInner<T>>,
    backlog: AtomicUsize,
    capacity: usize,
    eviction: Eviction,
}

struct SlotInner<T> {
    queue: VecDeque<T>,
    waker: Option<Waker>,
    open: bool,
}

impl<T> Slot<T> {
    fn new(capacity: usize, eviction: Eviction) -> Self {
        Self {
            inner: Mutex::new(SlotInner {
                queue: VecDeque::with_capacity(capacity.min(64)),
                waker: None,
                open: true,
            }),
            backlog: AtomicUsize::new(0),
            capacity,
            eviction,
        }
    }

    fn push(&self, item: T) -> Result<(Option<T>, Option<Waker>), T> {
        let mut inner = self.inner.lock();
        if !inner.open {
            return Err(item);
        }

        let evicted = if inner.queue.len() >= self.capacity {
            match self.eviction {
                Eviction::Front => inner.queue.pop_front(),
                Eviction::Back => inner.queue.pop_back(),
            }
        } else {
            self.backlog.fetch_add(1, Ordering::Relaxed);
            None
        };

        inner.queue.push_back(item);
        let waker = inner.waker.take();
        Ok((evicted, waker))
    }

    fn backlog(&self) -> usize {
        self.backlog.load(Ordering::Relaxed)
    }

    fn close(&self) -> Option<Waker> {
        let mut inner = self.inner.lock();
        inner.open = false;
        inner.waker.take()
    }
}

// ── Sender ──────────────────────────────────────────────────────────────────

/// Sender half of the acceptor channel.
///
/// Each sender caches the receiver slot list locally and refreshes only when
/// receivers are added or removed (epoch change).
pub struct Sender<T> {
    shared: Arc<Shared<T>>,
    cached_slots: Vec<Arc<Slot<T>>>,
    cached_epoch: usize,
    rng: Rng,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.shared.sender_count.fetch_add(1, Ordering::Relaxed);
        Self {
            shared: self.shared.clone(),
            cached_slots: self.cached_slots.clone(),
            cached_epoch: self.cached_epoch,
            rng: Rng::new(),
        }
    }
}

impl<T> Sender<T> {
    /// Send an item to the least-loaded receiver.
    ///
    /// Returns `Ok((evicted, waker))`:
    /// - `evicted`: item displaced from a full queue, caller should reset it
    /// - `waker`: if the receiver was parked, caller should forward to waker thread
    ///
    /// Returns `Err(item)` if there are no receivers.
    pub fn send(&mut self, item: T) -> Result<(Option<T>, Option<Waker>), T> {
        self.refresh_cache();

        if self.cached_slots.is_empty() {
            return Err(item);
        }

        let idx = pick_index(&mut self.rng, &self.cached_slots);
        self.cached_slots[idx].push(item)
    }

    fn refresh_cache(&mut self) {
        let current_epoch = self.shared.epoch.load(Ordering::Acquire);
        if self.cached_epoch != current_epoch {
            self.cached_slots = self.shared.slots.lock().clone();
            self.cached_epoch = current_epoch;
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.shared.sender_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            let slots = self.shared.slots.lock();
            for slot in slots.iter() {
                if let Some(w) = slot.close() {
                    w.wake();
                }
            }
        }
    }
}

fn pick_index<T>(rng: &mut Rng, slots: &[Arc<Slot<T>>]) -> usize {
    let len = slots.len();
    if len == 1 {
        return 0;
    }

    let a = rng.next_usize(len);
    let mut b = rng.next_usize(len - 1);
    if b >= a {
        b += 1;
    }

    let load_a = slots[a].backlog();
    let load_b = slots[b].backlog();

    if load_a <= load_b {
        a
    } else {
        b
    }
}

// ── Receiver ────────────────────────────────────────────────────────────────

/// Receiver half of the acceptor channel.
///
/// Cloning creates a new independent slot that will receive a share of future
/// sends. The receiver maintains a local buffer and swaps from the shared slot
/// only when empty, minimizing lock contention.
///
/// Slots are registered lazily on the first poll, so unpolled receivers never
/// participate in load balancing.
pub struct Receiver<T> {
    slot: Arc<Slot<T>>,
    shared: Arc<Shared<T>>,
    local: VecDeque<T>,
    registered: bool,
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self::new_unregistered(&self.shared)
    }
}

impl<T> Receiver<T> {
    fn new_unregistered(shared: &Arc<Shared<T>>) -> Self {
        let slot = Arc::new(Slot::new(shared.config.capacity, shared.config.eviction));
        shared.receiver_count.fetch_add(1, Ordering::Relaxed);
        Self {
            slot,
            shared: shared.clone(),
            local: VecDeque::new(),
            registered: false,
        }
    }

    fn ensure_registered(&mut self) {
        if !self.registered {
            self.shared.slots.lock().push(self.slot.clone());
            self.shared.epoch.fetch_add(1, Ordering::Release);
            self.registered = true;
        }
    }

    /// Try to receive an item without blocking.
    pub fn try_recv(&mut self) -> Option<T> {
        self.ensure_registered();
        if let Some(item) = self.local.pop_front() {
            self.slot.backlog.fetch_sub(1, Ordering::Relaxed);
            return Some(item);
        }
        self.swap_from_slot();
        let item = self.local.pop_front()?;
        self.slot.backlog.fetch_sub(1, Ordering::Relaxed);
        Some(item)
    }

    /// Poll for the next item.
    ///
    /// Registers the waker if the queue is empty so the sender can wake us later.
    pub fn poll_recv(&mut self, cx: &mut core::task::Context<'_>) -> Poll<Option<T>> {
        self.ensure_registered();
        if let Some(item) = self.local.pop_front() {
            self.slot.backlog.fetch_sub(1, Ordering::Relaxed);
            return Poll::Ready(Some(item));
        }

        let mut inner = self.slot.inner.lock();
        if !inner.queue.is_empty() {
            core::mem::swap(&mut self.local, &mut inner.queue);
            drop(inner);
            let item = self.local.pop_front();
            self.slot.backlog.fetch_sub(1, Ordering::Relaxed);
            return Poll::Ready(item);
        }
        if !inner.open {
            return Poll::Ready(None);
        }
        if inner
            .waker
            .as_ref()
            .map_or(true, |w| !w.will_wake(cx.waker()))
        {
            inner.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }

    /// Returns the current backlog (approximate).
    pub fn backlog(&self) -> usize {
        self.slot.backlog()
    }

    /// Whether the channel is closed (all senders dropped).
    pub fn is_closed(&self) -> bool {
        !self.slot.inner.lock().open
    }

    /// Receive an item, waiting asynchronously if none are available.
    ///
    /// Returns `None` when the channel is closed.
    pub async fn recv(&mut self) -> Option<T> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    fn swap_from_slot(&mut self) {
        debug_assert!(self.local.is_empty());
        let mut inner = self.slot.inner.lock();
        if !inner.queue.is_empty() {
            core::mem::swap(&mut self.local, &mut inner.queue);
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.registered {
            let mut slots = self.shared.slots.lock();
            slots.retain(|s| !Arc::ptr_eq(s, &self.slot));
            self.shared.epoch.fetch_add(1, Ordering::Release);
        }
        self.shared.receiver_count.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::Context;
    use std::{sync::Arc, task::Wake};

    struct NoopWaker;
    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }

    fn noop_waker() -> Waker {
        Arc::new(NoopWaker).into()
    }

    #[test]
    fn basic_send_recv() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (mut sender, mut rx) = new::<u32>(config);

        // Receiver must be polled to register its slot
        assert_eq!(rx.try_recv(), None);

        let (evicted, waker) = sender.send(42).unwrap();
        assert!(evicted.is_none());
        assert!(waker.is_none());

        assert_eq!(rx.try_recv(), Some(42));
        assert_eq!(rx.try_recv(), None);
    }

    #[test]
    fn eviction_front() {
        let config = Config {
            capacity: 2,
            eviction: Eviction::Front,
        };
        let (mut sender, mut rx) = new::<u32>(config);

        // Register the slot
        assert_eq!(rx.try_recv(), None);

        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let (evicted, _) = sender.send(3).unwrap();
        assert_eq!(evicted, Some(1));

        assert_eq!(rx.try_recv(), Some(2));
        assert_eq!(rx.try_recv(), Some(3));
    }

    #[test]
    fn eviction_back() {
        let config = Config {
            capacity: 2,
            eviction: Eviction::Back,
        };
        let (mut sender, mut rx) = new::<u32>(config);

        // Register the slot
        assert_eq!(rx.try_recv(), None);

        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let (evicted, _) = sender.send(3).unwrap();
        assert_eq!(evicted, Some(2));

        assert_eq!(rx.try_recv(), Some(1));
        assert_eq!(rx.try_recv(), Some(3));
    }

    #[test]
    fn waker_returned_when_parked() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (mut sender, mut rx) = new::<u32>(config);

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        // poll_recv registers the slot and parks
        assert!(rx.poll_recv(&mut cx).is_pending());

        let (_, waker) = sender.send(1).unwrap();
        assert!(waker.is_some());

        let (_, waker) = sender.send(2).unwrap();
        assert!(waker.is_none());
    }

    #[test]
    fn clone_receiver_scales() {
        let config = Config {
            capacity: 100,
            eviction: Eviction::Front,
        };
        let (mut sender, mut rx1) = new::<u32>(config);
        let mut rx2 = rx1.clone();
        let mut rx3 = rx1.clone();

        // Register all slots
        assert_eq!(rx1.try_recv(), None);
        assert_eq!(rx2.try_recv(), None);
        assert_eq!(rx3.try_recv(), None);

        for i in 0..100 {
            sender.send(i).unwrap();
        }

        let slots = sender.shared.slots.lock();
        let total: usize = slots.iter().map(|s| s.backlog()).sum();
        assert_eq!(total, 100);

        // No single receiver should have everything
        let max = slots.iter().map(|s| s.backlog()).max().unwrap();
        assert!(max < 80);
    }

    #[test]
    fn receiver_drop_unregisters() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (sender, mut rx1) = new::<u32>(config);
        let mut rx2 = rx1.clone();

        // Register both slots
        assert_eq!(rx1.try_recv(), None);
        assert_eq!(rx2.try_recv(), None);
        assert_eq!(sender.shared.slots.lock().len(), 2);

        drop(rx1);
        assert_eq!(sender.shared.slots.lock().len(), 1);

        drop(rx2);
        assert_eq!(sender.shared.slots.lock().len(), 0);
    }

    #[test]
    fn unpolled_receiver_not_registered() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (mut sender, rx1) = new::<u32>(config);
        let _rx2 = rx1.clone();

        // Neither receiver has been polled, so no slots are registered
        assert_eq!(sender.shared.slots.lock().len(), 0);
        assert!(sender.send(1).is_err());

        // Dropping unpolled receivers doesn't affect slot list
        drop(_rx2);
        drop(rx1);
        assert_eq!(sender.shared.slots.lock().len(), 0);
    }

    #[test]
    fn closed_on_sender_drop() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (mut sender, mut rx) = new::<u32>(config);

        // Register the slot
        assert_eq!(rx.try_recv(), None);

        sender.send(1).unwrap();
        drop(sender);

        assert_eq!(rx.try_recv(), Some(1));
        assert!(rx.is_closed());

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert_eq!(rx.poll_recv(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn sender_clone_keeps_channel_open() {
        let config = Config {
            capacity: 4,
            eviction: Eviction::Front,
        };
        let (sender, mut rx) = new::<u32>(config);
        let mut sender2 = sender.clone();

        drop(sender);

        // Channel still open because sender2 exists
        assert!(!rx.is_closed());

        // try_recv registers the slot
        assert_eq!(rx.try_recv(), None);

        sender2.send(42).unwrap();
        assert_eq!(rx.try_recv(), Some(42));

        drop(sender2);
        assert!(rx.is_closed());
    }
}
