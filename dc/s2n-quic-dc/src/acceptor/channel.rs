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

/// Error returned by [`Sender::send`] when the item cannot be delivered.
#[derive(Debug)]
pub enum SendError<T> {
    /// All receivers have been dropped. The channel is permanently closed.
    Closed(T),
    /// Receivers exist but none have polled yet (no registered slots).
    NoSlots(T),
}

impl<T> SendError<T> {
    pub fn into_inner(self) -> T {
        match self {
            Self::Closed(item) | Self::NoSlots(item) => item,
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
    /// Returns `Err(SendError::Closed)` if all receivers have been dropped.
    /// Returns `Err(SendError::NoSlots)` if receivers exist but none have polled yet.
    pub fn send(&mut self, item: T) -> Result<(Option<T>, Option<Waker>), SendError<T>> {
        self.refresh_cache();

        if self.cached_slots.is_empty() {
            if self.shared.receiver_count.load(Ordering::Relaxed) == 0 {
                return Err(SendError::Closed(item));
            }
            return Err(SendError::NoSlots(item));
        }

        let idx = pick_index(&mut self.rng, &self.cached_slots);
        self.cached_slots[idx].push(item).map_err(SendError::Closed)
    }

    /// Returns true if all receivers have been dropped.
    pub fn is_closed(&self) -> bool {
        self.shared.receiver_count.load(Ordering::Relaxed) == 0
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

    /// Eagerly register this receiver's slot so it participates in load
    /// balancing immediately, without requiring a first poll.
    pub fn register(&mut self) {
        self.ensure_registered();
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
            .is_none_or(|w| !w.will_wake(cx.waker()))
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
    use crate::testing::{ext::*, sim};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    // ── Helper ───────────────────────────────────────────────────────────────────

    /// Send an item and immediately fire any waker returned by the channel, so
    /// parked receiver tasks are rescheduled within the same bach step.
    fn send_wake<T>(sender: &mut Sender<T>, item: T) -> Result<Option<T>, T> {
        let (evicted, waker) = sender.send(item).map_err(|e| e.into_inner())?;
        if let Some(w) = waker {
            w.wake();
        }
        Ok(evicted)
    }

    // ── Tests ────────────────────────────────────────────────────────────────────

    /// A receiver that parks before any item is sent is woken when the sender
    /// pushes an item.  After the sender drops the channel closes and the
    /// receiver sees `None`.
    #[test]
    fn send_wakes_parked_receiver() {
        sim(|| {
            let (mut sender, mut rx) = new::<u32>(4.into());

            async move {
                let item = rx.recv().await;
                assert_eq!(
                    item,
                    Some(1),
                    "parked receiver should be woken with the sent item"
                );
                // Negative: channel closes after sender drops
                assert!(
                    rx.recv().await.is_none(),
                    "receiver should see None once the channel closes"
                );
            }
            .primary()
            .spawn();

            async move {
                1.ms().sleep().await; // let receiver park first
                send_wake(&mut sender, 1).unwrap();
                drop(sender);
            }
            .primary()
            .spawn();
        });
    }

    /// The first send to a parked receiver consumes the stored waker; the second
    /// send (before the receiver re-polls) finds no waker and returns `None`.
    #[test]
    fn second_send_does_not_return_waker() {
        sim(|| {
            let (mut sender, mut rx) = new::<u32>(4.into());

            async move {
                // Park the receiver so the sender can pick up its waker
                1.ms().sleep().await;

                assert_eq!(rx.recv().await, Some(1), "first item should arrive");
                assert_eq!(rx.recv().await, Some(2), "second item should arrive");
                assert!(rx.recv().await.is_none(), "channel should close");
            }
            .primary()
            .spawn();

            async move {
                1.ms().sleep().await;

                let (_, w1) = sender.send(1).unwrap();
                assert!(
                    w1.is_some(),
                    "first send to parked receiver should return a waker"
                );
                if let Some(w) = w1 {
                    w.wake();
                }

                // Second send before the receiver re-parks: waker slot is empty
                let (_, w2) = sender.send(2).unwrap();
                assert!(
                    w2.is_none(),
                    "second consecutive send should not return a waker"
                );

                drop(sender);
            }
            .primary()
            .spawn();
        });
    }

    /// When the last sender drops, a receiver that is waiting asynchronously
    /// receives `None`.
    #[test]
    fn channel_closes_when_last_sender_drops() {
        sim(|| {
            let (sender, mut rx) = new::<u32>(4.into());

            async move {
                assert!(
                    rx.recv().await.is_none(),
                    "receiver should see None when the channel closes"
                );
            }
            .primary()
            .spawn();

            async move {
                1.ms().sleep().await;
                drop(sender);
            }
            .primary()
            .spawn();
        });
    }

    /// Cloning a sender keeps the channel alive.  The receiver only sees `None`
    /// after every sender clone has been dropped.
    #[test]
    fn sender_clone_keeps_channel_open() {
        sim(|| {
            let (sender, mut rx) = new::<u32>(4.into());
            let mut sender2 = sender.clone();

            async move {
                let item = rx.recv().await;
                assert_eq!(item, Some(42), "item from cloned sender should arrive");
                assert!(
                    rx.recv().await.is_none(),
                    "channel should close only after both senders are gone"
                );
            }
            .primary()
            .spawn();

            async move {
                // Drop the original; the channel must stay open because sender2 lives
                drop(sender);
                1.ms().sleep().await;

                send_wake(&mut sender2, 42).unwrap();
                drop(sender2);
            }
            .primary()
            .spawn();
        });
    }

    /// With Front eviction, sending to a full queue discards the oldest item and
    /// delivers the newest two to the receiver.
    #[test]
    fn eviction_front_drops_oldest_item() {
        sim(|| {
            let (mut sender, mut rx) = new::<u32>(Config {
                capacity: 2,
                eviction: Eviction::Front,
            });

            async move {
                assert_eq!(
                    rx.recv().await,
                    Some(2),
                    "oldest item should have been evicted"
                );
                assert_eq!(rx.recv().await, Some(3), "newest item should be present");
                assert!(rx.recv().await.is_none());
            }
            .primary()
            .spawn();

            async move {
                1.ms().sleep().await;

                // Fill the queue and overflow once
                send_wake(&mut sender, 1).unwrap(); // will be evicted
                send_wake(&mut sender, 2).unwrap();
                let evicted = send_wake(&mut sender, 3).unwrap();
                assert_eq!(evicted, Some(1), "front-eviction should discard item 1");

                drop(sender);
            }
            .primary()
            .spawn();
        });
    }

    /// With Back eviction, sending to a full queue discards the most-recently
    /// queued item and preserves the older ones.
    #[test]
    fn eviction_back_drops_newest_item() {
        sim(|| {
            let (mut sender, mut rx) = new::<u32>(Config {
                capacity: 2,
                eviction: Eviction::Back,
            });

            async move {
                assert_eq!(
                    rx.recv().await,
                    Some(1),
                    "first item should survive back eviction"
                );
                assert_eq!(
                    rx.recv().await,
                    Some(3),
                    "overflow item should replace the back"
                );
                assert!(rx.recv().await.is_none());
            }
            .primary()
            .spawn();

            async move {
                1.ms().sleep().await;

                send_wake(&mut sender, 1).unwrap();
                send_wake(&mut sender, 2).unwrap(); // will be evicted
                let evicted = send_wake(&mut sender, 3).unwrap();
                assert_eq!(evicted, Some(2), "back-eviction should discard item 2");

                drop(sender);
            }
            .primary()
            .spawn();
        });
    }

    /// Three receivers each consume items concurrently.  Pick-two load balancing
    /// ensures no single receiver handles all the work.
    #[test]
    fn three_receivers_distribute_load() {
        let rx1_count = Arc::new(AtomicUsize::new(0));
        let rx2_count = Arc::new(AtomicUsize::new(0));
        let rx3_count = Arc::new(AtomicUsize::new(0));

        {
            let rx1_count = rx1_count.clone();
            let rx2_count = rx2_count.clone();
            let rx3_count = rx3_count.clone();

            sim(move || {
                let (mut sender, rx1) = new::<u32>(100.into());
                let rx2 = rx1.clone();
                let rx3 = rx1.clone();

                for (mut rx, count) in [(rx1, rx1_count), (rx2, rx2_count), (rx3, rx3_count)] {
                    async move {
                        while rx.recv().await.is_some() {
                            count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    .primary()
                    .spawn();
                }

                async move {
                    1.ms().sleep().await; // let all three receivers register

                    for i in 0u32..60 {
                        send_wake(&mut sender, i).unwrap();
                    }
                    drop(sender);
                }
                .primary()
                .spawn();
            });
        }

        let c1 = rx1_count.load(Ordering::Relaxed);
        let c2 = rx2_count.load(Ordering::Relaxed);
        let c3 = rx3_count.load(Ordering::Relaxed);

        assert_eq!(
            c1 + c2 + c3,
            60,
            "all 60 items should be received exactly once (distribution: rx1={c1}, rx2={c2}, rx3={c3})"
        );
        // Pick-two load balancing must spread the work: no receiver should be idle
        assert!(
            c1 > 0,
            "receiver 1 should receive some items (distribution: rx1={c1}, rx2={c2}, rx3={c3})"
        );
        assert!(
            c2 > 0,
            "receiver 2 should receive some items (distribution: rx1={c1}, rx2={c2}, rx3={c3})"
        );
        assert!(
            c3 > 0,
            "receiver 3 should receive some items (distribution: rx1={c1}, rx2={c2}, rx3={c3})"
        );
        // And no single receiver should be overwhelmed
        assert!(c1 < 50, "receiver 1 should not handle most of the load (distribution: rx1={c1}, rx2={c2}, rx3={c3})");
        assert!(c2 < 50, "receiver 2 should not handle most of the load (distribution: rx1={c1}, rx2={c2}, rx3={c3})");
        assert!(c3 < 50, "receiver 3 should not handle most of the load (distribution: rx1={c1}, rx2={c2}, rx3={c3})");
    }

    /// After one of two *registered* receivers is dropped its slot is removed.
    /// All subsequent sends go to the surviving receiver.
    ///
    /// Registration is lazy: a receiver only participates in load balancing
    /// after it has polled at least once.  This test:
    ///   1. Registers `rx2` via `try_recv` (which calls `ensure_registered`).
    ///   2. Drops `rx2`, removing its slot from the shared list.
    ///   3. Sends 10 items — they must all arrive on `rx1`.
    #[test]
    fn dropped_receiver_stops_receiving() {
        sim(|| {
            let (mut sender, mut rx1) = new::<u32>(100.into());
            let mut rx2 = rx1.clone();

            // Register rx2 by calling try_recv (returns None on empty queue),
            // then immediately drop it so its slot is removed.
            async move {
                let _ = rx2.try_recv(); // registers the slot, returns None
                                        // rx2 is dropped here — slot is unregistered
            }
            .spawn();

            async move {
                // Wait for the rx2 task to complete and unregister before sending.
                1.ms().sleep().await;

                for i in 0u32..10 {
                    send_wake(&mut sender, i).unwrap();
                }
                drop(sender);
            }
            .primary()
            .spawn();

            async move {
                let mut received = 0u32;
                while rx1.recv().await.is_some() {
                    received += 1;
                }
                assert_eq!(
                    received, 10,
                    "all items should arrive on the surviving receiver"
                );
            }
            .primary()
            .spawn();
        });
    }

    /// `send` returns `Err` when no receiver has polled yet (no registered slots).
    #[test]
    fn send_fails_with_no_registered_receivers() {
        sim(|| {
            async move {
                let (mut sender, rx) = new::<u32>(4.into());
                let _rx2 = rx.clone();

                // Neither receiver has polled — no slots registered
                assert!(
                    sender.send(1).is_err(),
                    "send should fail when no slots are registered"
                );

                // Dropping unpolled receivers must not change the slot count
                drop(_rx2);
                drop(rx);
                assert!(
                    sender.send(2).is_err(),
                    "send should still fail after unpolled receivers are dropped"
                );
            }
            .primary()
            .spawn();
        });
    }

    /// Demonstrates the unpolled-receiver backlog problem.
    ///
    /// Pattern: application spawns N tasks, cloning the receiver on each iteration.
    /// The original receiver is never polled (and not dropped). Without lazy registration,
    /// items would pile up in the original receiver's slot and get evicted. With lazy
    /// registration, only the polled clones participate in load balancing.
    #[test]
    fn unpolled_original_receiver_does_not_accumulate_backlog() {
        let evicted = Arc::new(AtomicUsize::new(0));

        {
            let evicted = evicted.clone();
            sim(move || {
                let (mut sender, rx) = new::<u32>(Config {
                    capacity: 4,
                    eviction: Eviction::Front,
                });

                // Simulate application pattern: clone into N workers, forget to drop original.
                // The original `rx` is pushed into `workers` so it stays alive (held but
                // never polled) for the entire test duration.
                let mut workers: Vec<Receiver<u32>> = vec![rx.clone()];
                for _ in 0..3 {
                    let mut worker_rx = rx.clone();
                    // Each worker polls immediately (registers its slot)
                    async move { while worker_rx.recv().await.is_some() {} }
                        .primary()
                        .spawn();
                }
                // Original `rx` goes into workers — NOT polled, NOT dropped until the
                // async block below drops `workers`.
                workers.push(rx);

                let evicted = evicted.clone();
                async move {
                    1.ms().sleep().await; // let workers register

                    // Send more items than the capacity of a single slot
                    for i in 0u32..20 {
                        match sender.send(i) {
                            Ok((Some(_), waker)) => {
                                evicted.fetch_add(1, Ordering::Relaxed);
                                if let Some(w) = waker {
                                    w.wake();
                                }
                            }
                            Ok((None, waker)) => {
                                if let Some(w) = waker {
                                    w.wake();
                                }
                            }
                            Err(_) => panic!("send should not fail"),
                        }
                    }
                    drop(sender);
                    drop(workers);
                }
                .primary()
                .spawn();
            });
        }

        // With lazy registration, the unpolled original receiver never gets a slot,
        // so all 20 items are distributed among the 3 active workers (capacity 4 each = 12).
        // Some eviction is expected since 20 > 12, but NOT from an idle receiver's queue.
        // The key assertion: eviction count should be much less than if all 20 items went
        // to a single never-drained slot (which would evict 16 out of 20).
        let evictions = evicted.load(Ordering::Relaxed);
        assert!(
            evictions <= 12,
            "evictions ({evictions}) should reflect active-worker capacity overflow, \
             not a single unpolled receiver accumulating all items"
        );
    }
}
