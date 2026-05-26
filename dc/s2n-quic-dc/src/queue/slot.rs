// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A single queue slot: two message halves plus an atomic binding identifier.
//!
//! The top bit (bit 63) of `binding_id` is the "unallocated" sentinel.  A slot
//! with that bit set is free for the allocator to claim.  All valid `VarInt`
//! binding IDs have the top two bits clear (QUIC VarInt encoding), so there is
//! no overlap.

use super::half::{self, Flags, Half, HalfInner};
use crate::{endpoint::msg, intrusive};
use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};
use s2n_quic_core::varint::VarInt;

/// The MSB of the u64 binding_id field is set when the slot is free.
pub(crate) const UNALLOCATED_BIT: u64 = 1 << 63;

/// Initial state: unallocated, no binding.
const UNALLOCATED: u64 = UNALLOCATED_BIT;

pub(crate) struct Slot {
    /// Packed field: MSB = unallocated flag, bits 0-62 = binding_id.
    ///
    /// All reads and writes are performed under at least one half lock.
    /// Writes (allocate/free) hold BOTH locks.  The Mutex provides ordering,
    /// so all atomic operations use Relaxed.
    binding_id: AtomicU64,
    /// The slot's position in the page table, fixed at creation time.
    queue_id: u64,
    pub(crate) stream: Half<msg::Stream>,
    pub(crate) control: Half<msg::Control>,
}

/// Result of `Slot::bind_and_push_stream`.
pub(crate) enum BindState {
    /// An existing, matching binding was found; the entry was pushed.
    AlreadyBound(half::AutoWake),
    /// A fresh binding was created; the entry was pushed.
    /// The caller must construct `StreamReceiver` / `ControlReceiver` and hand
    /// them to the stream handshake task.
    NewBinding(half::AutoWake),
}

impl Slot {
    /// Create a new, unallocated slot with its page-table index baked in.
    ///
    /// The initial stored binding is 0 (with UNALLOCATED_BIT set), so the first
    /// `bind_and_push_stream` must use a binding_id > 0.  Callers must ensure
    /// binding_ids start at 1 and increase monotonically per slot.
    pub(crate) fn with_queue_id(queue_id: VarInt) -> Self {
        Self {
            binding_id: AtomicU64::new(UNALLOCATED),
            queue_id: queue_id.as_u64(),
            stream: Half::new(),
            control: Half::new(),
        }
    }

    #[inline]
    pub(crate) fn queue_id(&self) -> VarInt {
        VarInt::new(self.queue_id).unwrap_or(VarInt::MAX)
    }

    /// Returns a stable raw pointer to this slot.
    ///
    /// SAFETY: the pointer is valid as long as the `Arc<State>` that owns
    /// the pinned page is kept alive.
    #[inline]
    pub(crate) fn as_ptr(&self) -> NonNull<Slot> {
        unsafe { NonNull::new_unchecked(self as *const Slot as *mut Slot) }
    }

    /// Mark the slot as unallocated (called while both half locks are held).
    ///
    /// Preserves the old binding_id value so that stale frames arriving after
    /// recycling can be distinguished from future-binding bugs.
    #[inline]
    pub(crate) fn mark_unallocated(&self) {
        let prev = self.binding_id.load(Ordering::Relaxed);
        self.binding_id.store(prev | UNALLOCATED_BIT, Ordering::Relaxed);
    }

    /// Push to the stream half, validating binding_id inside the lock.
    #[inline]
    pub(crate) fn push_stream(
        &self,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<half::AutoWake, super::Error<intrusive::Entry<msg::Stream>>> {
        let mut inner = self.stream.inner.lock();
        validate_and_push(binding_id, entry, &self.binding_id, &mut inner)
    }

    /// Push to the control half, validating binding_id inside the lock.
    #[inline]
    pub(crate) fn push_control(
        &self,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Control>,
    ) -> Result<half::AutoWake, super::Error<intrusive::Entry<msg::Control>>> {
        let mut inner = self.control.inner.lock();
        validate_and_push(binding_id, entry, &self.binding_id, &mut inner)
    }

    /// Set `binding_id` and open both receiver halves in one critical section.
    ///
    /// Acquires both half locks (stream → control) so that the binding store
    /// and the `HAS_RECEIVER` flag updates are never visible in a partial state.
    /// Returns `Err(Closed)` if the sender side has already been closed.
    #[inline]
    pub(crate) fn allocate_and_open(&self, binding_id: VarInt) -> Result<(), half::Closed> {
        let mut s = self.stream.inner.lock();
        let mut c = self.control.inner.lock();
        Self::allocate_and_open_locked(&mut s, &mut c, &self.binding_id, binding_id)
    }

    /// Bind the slot (if unallocated) and push the first stream entry atomically.
    ///
    /// All state transitions happen inside the combined stream+control lock so
    /// there is no window where `binding_id` is set but `HAS_RECEIVER` is not,
    /// and no window where two concurrent packets can both "win" a new binding.
    ///
    /// Returns:
    /// - `Ok(BindState::NewBinding(waker))` — slot was unallocated; caller
    ///   must create `StreamReceiver` / `ControlReceiver` and route them.
    /// - `Ok(BindState::AlreadyBound(waker))` — existing matching binding;
    ///   entry pushed normally.
    /// - `Err(_)` — stale binding, sender closed, or half closed.
    pub(crate) fn bind_and_push_stream(
        &self,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<BindState, super::Error<intrusive::Entry<msg::Stream>>> {
        let mut s = self.stream.inner.lock();
        let mut c = self.control.inner.lock();

        if !s.flags.contains(Flags::HAS_SENDER) {
            return Err(super::Error::SenderClosed);
        }

        let raw = self.binding_id.load(Ordering::Relaxed);
        let stored = raw & !UNALLOCATED_BIT;
        let incoming = binding_id.as_u64();

        if raw & UNALLOCATED_BIT != 0 {
            // Slot is free.  Only accept bindings strictly greater than the
            // previous value — this rejects stale frames from old generations.
            if incoming <= stored {
                return Err(super::Error::StaleBinding(entry));
            }
            Self::allocate_and_open_locked(&mut s, &mut c, &self.binding_id, binding_id)
                .map_err(|_| super::Error::SenderClosed)?;

            s.queue.push_back(entry);
            let waker = s.take_waker();
            return Ok(BindState::NewBinding(waker));
        }

        // Slot is allocated — classify the binding relationship.
        if incoming < stored {
            return Err(super::Error::StaleBinding(entry));
        }
        if incoming > stored {
            return Err(super::Error::FutureBinding(entry));
        }

        // Binding matches.
        if !s.flags.contains(Flags::HAS_RECEIVER) {
            return Err(super::Error::HalfClosed(entry));
        }

        s.queue.push_back(entry);
        let waker = s.take_waker();
        Ok(BindState::AlreadyBound(waker))
    }

    /// Broadcast-close both halves: clears HAS_SENDER, wakes receivers.
    ///
    /// Always locks — no fast-path skip.  On unallocated slots HAS_SENDER is
    /// already clear, so this is a no-op.
    pub(crate) fn broadcast_close(&self) -> (half::AutoWake, half::AutoWake) {
        let stream_wake = self.stream.broadcast_close();
        let control_wake = self.control.broadcast_close();
        (stream_wake, control_wake)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Store `binding_id` and set `HAS_RECEIVER` on both halves while both
    /// half locks are already held.  Returns `Err(Closed)` if either sender
    /// is gone.
    fn allocate_and_open_locked(
        s: &mut HalfInner<msg::Stream>,
        c: &mut HalfInner<msg::Control>,
        binding_id_cell: &AtomicU64,
        binding_id: VarInt,
    ) -> Result<(), half::Closed> {
        if !s.flags.contains(Flags::HAS_SENDER) || !c.flags.contains(Flags::HAS_SENDER) {
            return Err(half::Closed);
        }
        debug_assert!(
            !s.flags.contains(Flags::HAS_RECEIVER) && !c.flags.contains(Flags::HAS_RECEIVER),
            "receivers already open"
        );
        // Safe to use Relaxed here: the mutex release acts as the memory fence.
        binding_id_cell.store(binding_id.as_u64(), Ordering::Relaxed);
        s.flags.insert(Flags::HAS_RECEIVER);
        c.flags.insert(Flags::HAS_RECEIVER);
        Ok(())
    }
}

#[inline]
fn validate_and_push<T>(
    binding_id: VarInt,
    entry: intrusive::Entry<T>,
    slot_binding: &AtomicU64,
    inner: &mut HalfInner<T>,
) -> Result<half::AutoWake, super::Error<intrusive::Entry<T>>> {
    let raw = slot_binding.load(Ordering::Relaxed);
    let stored = raw & !UNALLOCATED_BIT;
    let incoming = binding_id.as_u64();

    if raw & UNALLOCATED_BIT != 0 {
        // Slot is free.  Compare against the last binding to classify the error.
        if incoming <= stored {
            return Err(super::Error::StaleBinding(entry));
        }
        return Err(super::Error::Unallocated(entry));
    }

    if incoming < stored {
        return Err(super::Error::StaleBinding(entry));
    }
    if incoming > stored {
        return Err(super::Error::FutureBinding(entry));
    }

    // binding matches
    if !inner.flags.contains(Flags::HAS_SENDER) {
        return Err(super::Error::SenderClosed);
    }
    if !inner.flags.contains(Flags::HAS_RECEIVER) {
        return Err(super::Error::HalfClosed(entry));
    }
    inner.queue.push_back(entry);
    Ok(inner.take_waker())
}

impl core::fmt::Debug for Slot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let raw = self.binding_id.load(Ordering::Relaxed);
        let binding = if raw & UNALLOCATED_BIT != 0 {
            None
        } else {
            VarInt::new(raw).ok()
        };
        f.debug_struct("Slot")
            .field("queue_id", &self.queue_id())
            .field("binding_id", &binding)
            .field("stream", &self.stream)
            .field("control", &self.control)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::testing::*;

    fn v(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    #[test]
    fn new_slot_is_unallocated() {
        let slot = Slot::with_queue_id(v(5));
        assert_eq!(slot.queue_id(), v(5));
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_ne!(raw & UNALLOCATED_BIT, 0);
    }

    #[test]
    fn allocate_and_open_sets_binding() {
        let slot = Slot::with_queue_id(v(0));
        assert!(slot.allocate_and_open(v(1)).is_ok());
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_eq!(raw, 1);
        assert!(slot.stream.inner.lock().flags.contains(Flags::HAS_RECEIVER));
        assert!(slot.control.inner.lock().flags.contains(Flags::HAS_RECEIVER));
    }

    #[test]
    fn allocate_and_open_fails_after_close() {
        let slot = Slot::with_queue_id(v(0));
        slot.broadcast_close();
        assert!(slot.allocate_and_open(v(1)).is_err());
    }

    #[test]
    fn push_stream_matching_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let result = slot.push_stream(v(1), make_stream_entry());
        assert!(result.is_ok());
    }

    #[test]
    fn push_stream_stale_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        let result = slot.push_stream(v(3), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn push_stream_future_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        let result = slot.push_stream(v(7), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::FutureBinding(_))));
    }

    #[test]
    fn push_stream_to_unallocated_stale() {
        let slot = Slot::with_queue_id(v(0));
        // fresh slot stored=0 with UNALLOCATED_BIT, push with binding=0 → 0<=0 → StaleBinding
        let result = slot.push_stream(v(0), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn push_stream_to_unallocated_future() {
        let slot = Slot::with_queue_id(v(0));
        // fresh slot, push binding=1 → 1 > 0 but UNALLOCATED → Unallocated
        let result = slot.push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::Unallocated(_))));
    }

    #[test]
    fn push_control_matching_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let result = slot.push_control(v(1), make_control_entry());
        assert!(result.is_ok());
    }

    #[test]
    fn mark_unallocated_preserves_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        slot.mark_unallocated();
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_ne!(raw & UNALLOCATED_BIT, 0);
        assert_eq!(raw & !UNALLOCATED_BIT, 5);
    }

    #[test]
    fn mark_unallocated_rejects_old_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        slot.mark_unallocated();
        let result = slot.push_stream(v(5), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_fresh_slot_new_binding() {
        let slot = Slot::with_queue_id(v(0));
        let result = slot.bind_and_push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Ok(BindState::NewBinding(_))));
    }

    #[test]
    fn bind_and_push_already_bound_matching() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let result = slot.bind_and_push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Ok(BindState::AlreadyBound(_))));
    }

    #[test]
    fn bind_and_push_stale_on_allocated() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        let result = slot.bind_and_push_stream(v(3), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_future_on_allocated() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        let result = slot.bind_and_push_stream(v(7), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::FutureBinding(_))));
    }

    fn simulate_receiver_drop(slot: &Slot) {
        slot.stream.inner.lock().flags.remove(Flags::HAS_RECEIVER);
        slot.control.inner.lock().flags.remove(Flags::HAS_RECEIVER);
        slot.mark_unallocated();
    }

    #[test]
    fn bind_and_push_stale_on_recycled() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(4), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_equal_on_recycled() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(5), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_new_binding_after_recycle() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5)).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(6), make_stream_entry());
        assert!(matches!(result, Ok(BindState::NewBinding(_))));
    }

    #[test]
    fn bind_and_push_after_broadcast_close() {
        let slot = Slot::with_queue_id(v(0));
        slot.broadcast_close();
        let result = slot.bind_and_push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::SenderClosed)));
    }

    #[test]
    fn bind_and_push_half_closed() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        // Close stream receiver
        slot.stream.inner.lock().flags.remove(Flags::HAS_RECEIVER);
        let result = slot.bind_and_push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::HalfClosed(_))));
    }

    #[test]
    fn broadcast_close_both_halves() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let (sw, cw) = slot.broadcast_close();
        // Both are AutoWake (possibly empty since no waker was registered)
        drop(sw);
        drop(cw);
        // Subsequent push fails
        let result = slot.push_stream(v(1), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::SenderClosed)));
    }
}
