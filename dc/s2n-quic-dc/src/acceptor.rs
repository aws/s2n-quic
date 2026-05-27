// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Acceptor registry for routing incoming streams to registered channel-based handlers.
//!
//! The registry maps VarInt acceptor IDs to channel senders. The architecture separates
//! the write path (registration/cleanup) from the read path (dispatch):
//!
//! **Write path** (rare): `Mutex`-protected sorted entries, builds a dense snapshot,
//! bumps a generation counter.
//!
//! **Read path** (hot): each dispatch worker owns a `LocalRegistry` that checks one
//! `AtomicU64` (the generation), then does a direct `(id - base)` index into a local
//! dense vec of `Sender` clones. No locking, no hashing, no scanning.
//!
//! **Cleanup**: when a `send()` fails because all receivers dropped, the local marks
//! the slot stale and bumps a `needs_cleanup` flag. A background task polls this flag
//! and rebuilds the snapshot without the dead entries.

pub mod channel;

use crate::flow::queue::AutoWake;
use channel::{Config, Receiver, Sender};
use core::task::Waker;
use s2n_quic_core::varint::VarInt;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

/// Immutable snapshot mapping acceptor IDs to channel senders via base-offset indexing.
struct Snapshot<T> {
    base: u64,
    slots: Vec<Option<Sender<T>>>,
}

impl<T> Snapshot<T> {
    fn empty() -> Self {
        Self {
            base: 0,
            slots: Vec::new(),
        }
    }
}

/// Mutable state protected by the write mutex. Entries are kept sorted by acceptor ID
/// so min/max are always front/back with no iteration.
struct WriteState<T> {
    entries: Vec<(u64, Sender<T>)>,
}

impl<T> WriteState<T> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn contains(&self, id: u64) -> bool {
        self.entries.binary_search_by_key(&id, |(k, _)| *k).is_ok()
    }

    fn insert(&mut self, id: u64, sender: Sender<T>) {
        let pos = self.entries.partition_point(|(k, _)| *k < id);
        self.entries.insert(pos, (id, sender));
        self.invariants();
    }

    fn cleanup(&mut self) -> usize {
        let mut removed = 0;
        self.entries.retain(|(_, sender)| {
            let keep = !sender.is_closed();
            if !keep {
                removed += 1;
            }
            keep
        });
        removed
    }

    fn build_snapshot(&self) -> Arc<Snapshot<T>> {
        if self.entries.is_empty() {
            return Arc::new(Snapshot::empty());
        }

        let min = self.entries.first().unwrap().0;
        let max = self.entries.last().unwrap().0;
        let len = (max - min + 1) as usize;

        let mut slots: Vec<Option<Sender<T>>> = vec![None; len];
        for (id, sender) in &self.entries {
            let idx = (*id - min) as usize;
            slots[idx] = Some(sender.clone());
        }

        Arc::new(Snapshot { base: min, slots })
    }

    fn invariants(&self) {
        if cfg!(test) {
            for i in 1..self.entries.len() {
                assert!(self.entries[i - 1].0 < self.entries[i].0);
            }
        }
    }
}

/// Shared state behind the registry, accessible by both the write path and background cleanup.
struct Shared<T> {
    /// Latest snapshot, protected by a lightweight lock. Only accessed on generation mismatch
    /// (rare) or during writes. The fast path never touches this.
    snapshot: Mutex<Arc<Snapshot<T>>>,
    generation: AtomicU64,
    /// Cleaner state: waker + dirty flag. Only locked on channel closure (rare) and
    /// when the cleaner task polls. Never touched on the normal send fast path.
    cleaner: Mutex<CleanerState>,
    write: Mutex<WriteState<T>>,
}

struct CleanerState {
    waker: Option<Waker>,
    needs_cleanup: bool,
}

impl<T: Send + 'static> Shared<T> {
    fn cleanup_closed(&self) {
        let snapshot = {
            let mut state = self.write.lock().unwrap();
            let removed = state.cleanup();
            if removed > 0 {
                Some(state.build_snapshot())
            } else {
                None
            }
        };
        if let Some(snapshot) = snapshot {
            *self.snapshot.lock().unwrap() = snapshot;
            self.generation.fetch_add(1, Ordering::Release);
        }
    }
}

/// Registry for managing acceptor channels.
///
/// Shared across the endpoint for registration and across dispatch workers (via `local()`).
pub struct Registry<T: Send + 'static> {
    shared: Arc<Shared<T>>,
}

impl<T: Send + 'static> Clone for Registry<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T: Send + 'static> Registry<T> {
    /// Create a new acceptor registry
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                snapshot: Mutex::new(Arc::new(Snapshot::empty())),
                generation: AtomicU64::new(0),
                cleaner: Mutex::new(CleanerState {
                    waker: None,
                    needs_cleanup: false,
                }),
                write: Mutex::new(WriteState::new()),
            }),
        }
    }

    /// Register a new acceptor channel and return a receiver.
    ///
    /// The acceptor is automatically cleaned up when all receivers are dropped
    /// and a background cleanup pass runs.
    ///
    /// Returns None if the acceptor_id is already registered.
    pub fn register(&self, acceptor_id: VarInt, config: Config) -> Option<Receiver<T>> {
        let (snapshot, receiver) = {
            let id = u64::from(acceptor_id);
            let mut state = self.shared.write.lock().unwrap();

            if state.contains(id) {
                return None;
            }

            let (sender, receiver) = channel::new(config);
            state.insert(id, sender);
            let snapshot = state.build_snapshot();
            (snapshot, receiver)
        };
        *self.shared.snapshot.lock().unwrap() = snapshot;
        self.shared.generation.fetch_add(1, Ordering::Release);

        Some(receiver)
    }

    /// Run deferred cleanup of closed channels.
    ///
    /// Call this periodically from a background task. The `needs_cleanup` flag
    /// is a fast-path hint from dispatch workers, but this also catches channels
    /// that closed without any send attempts (e.g., receiver dropped immediately).
    pub fn cleanup(&self) {
        self.shared.cleanup_closed();
    }

    /// Returns a future that performs background cleanup whenever a dispatch worker
    /// detects a closed channel.
    ///
    /// The returned `Cleaner` registers a waker on first poll, then wakes up each time
    /// a `LocalRegistry::send` encounters a closed channel (via the waker_sink pipeline).
    /// Spawn this on any async runtime — it works with tokio, bach, or busy-poll.
    pub fn cleaner(&self) -> Cleaner<T> {
        Cleaner {
            shared: self.shared.clone(),
        }
    }

    /// Create a per-worker local registry for lock-free dispatch.
    ///
    /// Each `LocalRegistry` caches its own `Sender` clones with independent
    /// pick-two RNG and slot caches.
    pub fn local(&self) -> LocalRegistry<T> {
        let snap = self.shared.snapshot.lock().unwrap().clone();
        let gen = self.shared.generation.load(Ordering::Acquire);
        let local_slots = snap.slots.clone();
        LocalRegistry {
            shared: self.shared.clone(),
            cached_generation: gen,
            base: snap.base,
            local_slots,
        }
    }
}

impl<T> Default for Registry<T>
where
    T: Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Background task that cleans up dead acceptor entries when woken.
///
/// This future never completes — it loops forever, sleeping until a dispatch
/// worker detects a closed channel and wakes it via the waker_sink pipeline.
pub struct Cleaner<T: Send + 'static> {
    shared: Arc<Shared<T>>,
}

impl<T: Send + 'static> std::future::Future for Cleaner<T> {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        let needs_cleanup = {
            let mut guard = self.shared.cleaner.lock().unwrap();
            let new_waker = cx.waker();
            if guard.waker.as_ref().is_none_or(|w| !w.will_wake(new_waker)) {
                guard.waker = Some(new_waker.clone());
            }
            std::mem::take(&mut guard.needs_cleanup)
        };

        if needs_cleanup {
            self.shared.cleanup_closed();
        }

        std::task::Poll::Pending
    }
}

/// Per-dispatch-worker handle providing lock-free access to acceptor senders.
///
/// The read path checks a single `AtomicU64` generation counter (one `Relaxed` load),
/// then indexes directly into a dense local vec by `(id - base)`. No locking, no hashing.
pub struct LocalRegistry<T> {
    shared: Arc<Shared<T>>,
    cached_generation: u64,
    base: u64,
    local_slots: Vec<Option<Sender<T>>>,
}

/// Result of sending a stream to an acceptor channel.
pub enum SendResult<T> {
    /// Acceptor not registered for this ID.
    NotFound,
    /// Channel is closed (all receivers dropped). The `AutoWake` should be forwarded
    /// to the waker_sink to trigger background cleanup of the dead registry entry.
    Closed(T, AutoWake),
    /// Receivers exist but none have polled yet (no registered slots).
    NoSlots(T),
    /// Successfully sent. Contains an evicted item (if queue was full) and a waker.
    Ok { evicted: Option<T>, waker: AutoWake },
}

impl<T> LocalRegistry<T> {
    /// Returns a mutable reference to the sender for the given acceptor ID,
    /// or `None` if not registered.
    #[inline]
    pub fn get(&mut self, acceptor_id: VarInt) -> Option<&mut Sender<T>> {
        if self.get_local(acceptor_id).is_none() {
            self.refresh();
        }
        self.get_local(acceptor_id)
    }

    /// Send a stream to the acceptor registered for `acceptor_id`.
    ///
    /// Fast path: one array index. No locks.
    #[inline]
    pub fn send(&mut self, acceptor_id: VarInt, item: T) -> SendResult<T> {
        let sender = match self.get_local(acceptor_id) {
            Some(s) => s,
            None => {
                // try refreshing if we're out of date
                self.refresh();
                match self.get_local(acceptor_id) {
                    Some(s) => s,
                    // If the slot went stale between the first and second lookup, treat as not found
                    None => return SendResult::NotFound,
                }
            }
        };

        match sender.send(item) {
            Ok((evicted, waker)) => SendResult::Ok {
                evicted,
                waker: AutoWake::new(waker),
            },
            Err(channel::SendError::Closed(item)) => {
                let raw = u64::from(acceptor_id);
                let idx = (raw - self.base) as usize;
                self.local_slots[idx] = None;
                let mut cleaner = self.shared.cleaner.lock().unwrap();
                cleaner.needs_cleanup = true;
                let waker = cleaner.waker.clone();
                SendResult::Closed(item, AutoWake::new(waker))
            }
            Err(channel::SendError::NoSlots(item)) => SendResult::NoSlots(item),
        }
    }

    #[inline]
    fn refresh(&mut self) {
        // Refresh the local snapshot if the generation has changed
        // Note that this is using relaxed since it's on the hot path and dispatchers
        // don't need the absolute latest of the acceptor mapping. Most acceptors
        // are static for the lifetime of the endpoint.
        let current_gen = self.shared.generation.load(Ordering::Relaxed);
        if current_gen != self.cached_generation {
            let snap = self.shared.snapshot.lock().unwrap().clone();
            self.base = snap.base;
            self.local_slots = snap.slots.clone();
            self.cached_generation = current_gen;
        }
    }

    #[inline]
    fn get_local(&mut self, acceptor_id: VarInt) -> Option<&mut Sender<T>> {
        let raw = u64::from(acceptor_id);
        let idx = raw.checked_sub(self.base)? as usize;
        self.local_slots.get_mut(idx)?.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_lookup() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let _rx = registry.register(acceptor_id, Config::default()).unwrap();

        // Cannot register same ID twice
        assert!(registry.register(acceptor_id, Config::default()).is_none());
    }

    #[tokio::test]
    async fn test_send_not_found() {
        let registry: Registry<String> = Registry::new();
        let mut local = registry.local();

        let result = local.send(VarInt::from_u8(99), "hello".into());
        assert!(matches!(result, SendResult::NotFound));
    }

    #[tokio::test]
    async fn test_send_before_poll_returns_no_slots() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let _rx = registry.register(acceptor_id, Config::default()).unwrap();
        let mut local = registry.local();

        // Receiver hasn't polled yet — no slots registered
        let result = local.send(acceptor_id, "hello".into());
        assert!(matches!(result, SendResult::NoSlots(_)));
    }

    #[tokio::test]
    async fn test_send_and_receive() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let mut rx = registry.register(acceptor_id, Config::default()).unwrap();
        // Trigger slot registration by polling once
        assert!(rx.try_recv().is_none());
        let mut local = registry.local();

        let result = local.send(acceptor_id, "hello".into());
        assert!(matches!(result, SendResult::Ok { evicted: None, .. }));

        let item = rx.try_recv().unwrap();
        assert_eq!(item, "hello");
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let rx = registry.register(acceptor_id, Config::default()).unwrap();
        drop(rx);
        registry.cleanup();

        let mut local = registry.local();
        let result = local.send(acceptor_id, "hello".into());
        assert!(matches!(result, SendResult::NotFound));
    }

    #[tokio::test]
    async fn test_closed_channel() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let rx = registry.register(acceptor_id, Config::default()).unwrap();
        drop(rx);

        let mut local = registry.local();
        let result = local.send(acceptor_id, "hello".into());
        assert!(matches!(result, SendResult::Closed(..)));
    }

    #[tokio::test]
    async fn test_closed_channel_triggers_cleanup() {
        let registry: Registry<String> = Registry::new();
        let acceptor_id = VarInt::from_u8(1);

        let rx = registry.register(acceptor_id, Config::default()).unwrap();
        drop(rx);

        let mut local = registry.local();
        let _ = local.send(acceptor_id, "hello".into());

        // Background cleanup removes the dead entry
        registry.cleanup();

        // After cleanup + refresh, it's NotFound
        let result = local.send(acceptor_id, "world".into());
        assert!(matches!(result, SendResult::NotFound));
    }

    #[tokio::test]
    async fn test_sparse_ids() {
        let registry: Registry<String> = Registry::new();

        let mut rx1 = registry
            .register(VarInt::from_u8(5), Config::default())
            .unwrap();
        let mut rx2 = registry
            .register(VarInt::from_u8(200), Config::default())
            .unwrap();

        // Register slots by polling
        assert!(rx1.try_recv().is_none());
        assert!(rx2.try_recv().is_none());

        let mut local = registry.local();

        let result = local.send(VarInt::from_u8(5), "five".into());
        assert!(matches!(result, SendResult::Ok { .. }));

        let result = local.send(VarInt::from_u8(200), "two hundred".into());
        assert!(matches!(result, SendResult::Ok { .. }));

        // Gap IDs should return NotFound
        let result = local.send(VarInt::from_u8(100), "gap".into());
        assert!(matches!(result, SendResult::NotFound));

        assert_eq!(rx1.try_recv().unwrap(), "five");
        assert_eq!(rx2.try_recv().unwrap(), "two hundred");
    }

    #[tokio::test]
    async fn test_unregister_does_not_break_others() {
        let registry: Registry<String> = Registry::new();

        let rx1 = registry
            .register(VarInt::from_u8(10), Config::default())
            .unwrap();
        let mut rx2 = registry
            .register(VarInt::from_u8(20), Config::default())
            .unwrap();

        // Register rx2's slot
        assert!(rx2.try_recv().is_none());

        drop(rx1);
        registry.cleanup();

        let mut local = registry.local();

        let result = local.send(VarInt::from_u8(10), "gone".into());
        assert!(matches!(result, SendResult::NotFound));

        let result = local.send(VarInt::from_u8(20), "still here".into());
        assert!(matches!(result, SendResult::Ok { .. }));

        assert_eq!(rx2.try_recv().unwrap(), "still here");
    }

    #[tokio::test]
    async fn test_local_cache_refreshes_on_new_registration() {
        let registry: Registry<String> = Registry::new();
        let mut local = registry.local();

        // Initially no acceptors
        let result = local.send(VarInt::from_u8(1), "early".into());
        assert!(matches!(result, SendResult::NotFound));

        // Register after local was created
        let mut rx = registry
            .register(VarInt::from_u8(1), Config::default())
            .unwrap();
        // Register the slot by polling
        assert!(rx.try_recv().is_none());

        // Local should pick up the new snapshot
        let result = local.send(VarInt::from_u8(1), "late".into());
        assert!(matches!(result, SendResult::Ok { .. }));

        assert_eq!(rx.try_recv().unwrap(), "late");
    }

    /// End-to-end bach test: when a receiver drops and a sender detects the closure,
    /// the cleaner waker fires, the cleaner task runs cleanup, and subsequent sends
    /// see `NotFound` instead of `Closed`.
    #[test]
    fn cleaner_removes_dead_entry_after_waker_fires() {
        use crate::testing::{ext::*, sim};

        sim(|| {
            let registry: Registry<u32> = Registry::new();
            let acceptor_id = VarInt::from_u8(1);

            let mut rx = registry.register(acceptor_id, 4.into()).unwrap();
            // Register the slot so sends succeed initially
            assert!(rx.try_recv().is_none());

            let mut local = registry.local();
            let cleaner = registry.cleaner();

            // Spawn the cleaner task
            async move {
                cleaner.await;
            }
            .spawn();

            async move {
                // Verify sends work before the receiver drops
                let result = local.send(acceptor_id, 1);
                assert!(matches!(result, SendResult::Ok { .. }));
                assert_eq!(rx.try_recv(), Some(1));

                // Drop the receiver — channel is now closed
                drop(rx);

                // Next send detects closure and returns Closed with a cleanup waker
                let result = local.send(acceptor_id, 2);
                let SendResult::Closed(item, mut waker) = result else {
                    panic!("expected Closed, got something else");
                };
                assert_eq!(item, 2);

                // Fire the cleanup waker (simulates what waker_sink + drain does)
                if let Some(w) = waker.take() {
                    w.wake();
                }

                // Yield to let the cleaner task run
                1.ms().sleep().await;

                // After cleanup, the entry is gone — sends now return NotFound
                let result = local.send(acceptor_id, 3);
                assert!(
                    matches!(result, SendResult::NotFound),
                    "expected NotFound after cleanup, got {:?}",
                    match &result {
                        SendResult::NotFound => "NotFound",
                        SendResult::Closed(..) => "Closed",
                        SendResult::NoSlots(_) => "NoSlots",
                        SendResult::Ok { .. } => "Ok",
                    }
                );
            }
            .primary()
            .spawn();
        });
    }
}
