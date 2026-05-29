// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Waker offload infrastructure: moves cross-thread wake syscalls off busy-poll dispatch threads.
//!
//! The recv dispatch and completion paths produce wakers after dispatching frames to flow queues.
//! Rather than calling `waker.wake()` inline (which blocks on futex/eventfd syscalls), producers
//! push wakers into a per-worker `Slot`. A separate drain task polls all slots and fires the
//! wakers in bulk, freeing the dispatch thread to process packets at line rate.
//!
//! Each producer gets its own `Slot` (mutex-guarded Vec) so there is no cross-producer contention.
//! The drain task swaps the Vec out in O(1) and iterates the local copy.

use crate::{queue::AutoWake, socket::channel, tracing::*};
use core::task::{Poll, Waker};
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// A mutex-guarded collection of wakers pending invocation.
///
/// One Slot exists per producer (dispatch worker or send worker). The producer pushes wakers
/// via `Sink`; the `Drain` receiver swaps the Vec out and yields wakers one at a time.
pub(crate) struct Slot {
    inner: Mutex<SlotInner>,
}

struct SlotInner {
    wakers: VecDeque<Waker>,
    drain_waker: Option<Waker>,
}

impl Slot {
    fn new() -> Self {
        Self {
            inner: Mutex::new(SlotInner {
                wakers: VecDeque::with_capacity(256),
                drain_waker: None,
            }),
        }
    }

    fn push(&self, waker: Waker) {
        let notify = {
            let mut guard = self.inner.lock();
            guard.wakers.push_back(waker);
            guard.drain_waker.take()
        };
        let has_notify = notify.is_some();
        trace!(has_drain_waker = has_notify, "waker::Slot::push");
        if let Some(w) = notify {
            w.wake();
        }
    }

    fn swap_into(&self, cx: &core::task::Context<'_>, out: &mut VecDeque<Waker>) {
        let mut guard = self.inner.lock();
        let swapped_count = guard.wakers.len();
        core::mem::swap(&mut guard.wakers, out);
        if guard
            .drain_waker
            .as_ref()
            .is_none_or(|w| !w.will_wake(cx.waker()))
        {
            guard.drain_waker = Some(cx.waker().clone());
        }
        trace!(swapped_count, "waker::Slot::swap_into");
    }
}

/// Handle given to a producer to push wakers without invoking them.
///
/// Cheap to clone (Arc bump). Each Sink targets exactly one Slot.
/// When all Sinks are dropped the Drain will shut down.
pub(crate) struct Sink {
    slot: Arc<Slot>,
    alive: Arc<SinkState>,
}

struct SinkState {
    count: AtomicUsize,
    drain_waker: Mutex<Option<Waker>>,
}

impl Clone for Sink {
    fn clone(&self) -> Self {
        self.alive.count.fetch_add(1, Ordering::Relaxed);
        Self {
            slot: self.slot.clone(),
            alive: self.alive.clone(),
        }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        if self.alive.count.fetch_sub(1, Ordering::Release) == 1 {
            std::sync::atomic::fence(Ordering::Acquire);
            if let Some(w) = self.alive.drain_waker.lock().take() {
                w.wake();
            }
        }
    }
}

impl channel::UnboundedSender<Waker> for Sink {
    fn send(&mut self, waker: Waker) -> Result<(), Waker> {
        self.slot.push(waker);
        Ok(())
    }
}

impl channel::UnboundedSender<AutoWake> for Sink {
    fn send(&mut self, mut auto_wake: AutoWake) -> Result<(), AutoWake> {
        if let Some(waker) = auto_wake.take() {
            trace!("waker::Sink::send(AutoWake) -> push");
            self.slot.push(waker);
        } else {
            trace!("waker::Sink::send(AutoWake) -> empty, skipped");
        }
        Ok(())
    }
}

/// Receiver side: polls all slots round-robin and yields wakers one at a time.
///
/// Implements `Receiver<Waker>` so it composes with `Map` + `drain_budgeted`.
/// Returns `None` (shutting down) once all `Sink`s have been dropped and all
/// buffered wakers have been drained.
pub(crate) struct Drain {
    slots: Arc<[Arc<Slot>]>,
    local: VecDeque<Waker>,
    slot_idx: usize,
    alive: Arc<SinkState>,
}

impl Drain {
    fn new(slots: Arc<[Arc<Slot>]>, alive: Arc<SinkState>) -> Self {
        Self {
            slots,
            local: VecDeque::with_capacity(256),
            slot_idx: 0,
            alive,
        }
    }
}

impl channel::Receiver<Waker> for Drain {
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut channel::Budget,
    ) -> Poll<Option<Waker>> {
        if budget.is_exhausted() {
            if !self.local.is_empty() {
                budget.set_needs_wake();
            }
            return Poll::Pending;
        }

        if let Some(waker) = self.local.pop_front() {
            budget.consume();
            trace!("waker::Drain::poll_recv -> Ready (from local)");
            return Poll::Ready(Some(waker));
        }

        let num_slots = self.slots.len();
        for _ in 0..num_slots {
            let slot = &self.slots[self.slot_idx];
            self.slot_idx = (self.slot_idx + 1) % num_slots;

            slot.swap_into(cx, &mut self.local);
            if let Some(waker) = self.local.pop_front() {
                budget.consume();
                trace!(
                    local_remaining = self.local.len(),
                    "waker::Drain::poll_recv -> Ready (from slot)"
                );
                return Poll::Ready(Some(waker));
            }
        }

        // All slots empty — check if all producers are gone.
        if self.alive.count.load(Ordering::Acquire) == 0 {
            return Poll::Ready(None);
        }

        // Register so the last Sink drop wakes us.
        let mut guard = self.alive.drain_waker.lock();
        if guard.as_ref().is_none_or(|w| !w.will_wake(cx.waker())) {
            *guard = Some(cx.waker().clone());
        }

        trace!("waker::Drain::poll_recv -> Pending");
        Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

/// Creates `num_slots` slots partitioned across `num_drains` drain tasks.
///
/// Returns (one Sink per slot, one Drain per drain task). Slots are distributed round-robin
/// across drains so each drain polls a roughly equal share. When all Sinks are dropped, the
/// Drains will shut down after flushing any remaining buffered wakers.
pub(crate) fn new(num_slots: usize, num_drains: usize) -> (Vec<Sink>, Vec<Drain>) {
    assert!(num_drains > 0, "at least one drain task is required");

    let alive = Arc::new(SinkState {
        count: AtomicUsize::new(num_slots),
        drain_waker: Mutex::new(None),
    });

    let slots: Vec<Arc<Slot>> = (0..num_slots).map(|_| Arc::new(Slot::new())).collect();
    let sinks: Vec<Sink> = slots
        .iter()
        .map(|s| Sink {
            slot: s.clone(),
            alive: alive.clone(),
        })
        .collect();

    let mut partitions: Vec<Vec<Arc<Slot>>> = (0..num_drains).map(|_| Vec::new()).collect();
    for (i, slot) in slots.into_iter().enumerate() {
        partitions[i % num_drains].push(slot);
    }

    let drains: Vec<Drain> = partitions
        .into_iter()
        .map(|p| Drain::new(p.into(), alive.clone()))
        .collect();

    (sinks, drains)
}
