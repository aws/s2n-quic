// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A single queue slot: two message halves plus an atomic binding identifier.
//!
//! The top bit (bit 63) of `binding_id` is the "unallocated" sentinel.  A slot
//! with that bit set is free for the allocator to claim.  All valid `VarInt`
//! binding IDs have the top two bits clear (QUIC VarInt encoding), so there is
//! no overlap.

use super::{
    half::{self, Flags, Half, HalfInner},
    msg_table::MsgTable,
};
use crate::tracing::*;
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
    pub(crate) stream: Half<msg::Stream, StreamState>,
    pub(crate) control: Half<msg::Control>,
}

pub(crate) struct StreamState {
    pub(crate) msg_table: Option<MsgTable>,
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
            stream: Half::with_extra(StreamState { msg_table: None }),
            control: Half::new(),
        }
    }

    #[inline]
    pub(crate) fn queue_id(&self) -> VarInt {
        VarInt::new(self.queue_id).unwrap_or(VarInt::MAX)
    }

    #[inline]
    pub(crate) fn binding_id_raw(&self) -> u64 {
        self.binding_id.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn binding_id(&self) -> VarInt {
        let raw = self.binding_id.load(Ordering::Relaxed);
        VarInt::new(raw & !UNALLOCATED_BIT).unwrap_or(VarInt::ZERO)
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
        self.binding_id
            .store(prev | UNALLOCATED_BIT, Ordering::Relaxed);
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

    /// Dispatch a QueueMsg frame to the message reassembly table.
    ///
    /// Validates binding_id, checks out the chunk, invokes `write_fn` to fill the
    /// buffer region (either a memcpy from decrypted payload or a direct scatter-decrypt),
    /// marks the chunk complete, and if the message (and all prior messages) are ready,
    /// pushes them into the stream half's queue and returns the waker.
    ///
    /// If `write_fn` returns `Err`, the checkout is cleared without marking received
    /// (the transport will retransmit).
    pub(crate) fn push_msg<E>(
        &self,
        binding_id: VarInt,
        msg_id: u64,
        stream_offset: u64,
        message_size: u32,
        chunk_size: u16,
        chunk_index: u32,
        payload_len: u32,
        is_fin: bool,
        is_wakeup: bool,
        write_fn: impl FnOnce(*mut u8, u32) -> Result<(), E>,
    ) -> Result<half::AutoWake, super::MsgError<E>> {
        // Validate + checkout under the stream lock.
        let (ptr, expected_len, chunk_index, keep_alive) = {
            let mut stream = self.stream.inner.lock();
            validate_msg_dispatch(binding_id, &self.binding_id, &stream)
                .map_err(super::MsgError::Queue)?;
            let table = stream.extra.msg_table.get_or_insert_with(MsgTable::new);

            match table.insert(
                msg_id,
                stream_offset,
                message_size,
                chunk_size,
                chunk_index,
                payload_len,
                is_fin,
                is_wakeup,
            ) {
                Ok(checkout) => (
                    checkout.ptr,
                    checkout.expected_len,
                    checkout.chunk_index,
                    checkout.keep_alive,
                ),
                Err(e) => {
                    trace!(msg_id, chunk_index, ?e, "slot::send_msg insert failed");
                    return Ok(half::AutoWake::default());
                }
            }
        };

        // Invoke the write callback (outside both locks).
        // For the mixed path: memcpy from decrypted BytesMut.
        // For the fast path: scatter-decrypt directly into ptr.
        let write_result = write_fn(ptr, expected_len);

        // Validate + complete/cancel under the stream lock.
        let mut stream = self.stream.inner.lock();
        let validation = validate_msg_dispatch(binding_id, &self.binding_id, &stream);
        if let Err(error) = write_result {
            if validation.is_ok() {
                if let Some(table) = stream.extra.msg_table.as_mut() {
                    table.cancel_checkout(msg_id, chunk_index);
                }
            }
            return Err(super::MsgError::Write(error));
        }

        if let Err(error) = validation {
            trace!(
                msg_id,
                chunk_index,
                ?error,
                "slot::send_msg completion validation failed"
            );
            if matches!(error, super::Error::HalfClosed(_) | super::Error::SenderClosed) {
                if let Some(table) = stream.extra.msg_table.as_mut() {
                    table.cancel_checkout(msg_id, chunk_index);
                }
            }
            return Ok(half::AutoWake::default());
        }
        let local_queue = {
            let Some(table) = stream.extra.msg_table.as_mut() else {
                return Ok(half::AutoWake::default());
            };
            match table.complete(msg_id, chunk_index) {
                super::msg_table::CompleteOutcome::Ready => {
                    let mut queue = intrusive::Queue::new();
                    let mut should_wake = false;
                    for delivered in table.drain_complete() {
                        should_wake |= delivered.is_wakeup;
                        let entry: intrusive::Entry<msg::Stream> = msg::Stream::Data {
                            offset: VarInt::new(delivered.stream_offset).unwrap_or(VarInt::MAX),
                            fin: delivered.is_fin,
                            payload: delivered.payload,
                        }
                        .into();
                        queue.push_back(entry);
                    }
                    Some((queue, should_wake))
                }
                super::msg_table::CompleteOutcome::Pending
                | super::msg_table::CompleteOutcome::Poisoned => None,
            }
        };

        drop(keep_alive);

        match local_queue {
            Some((mut queue, should_wake)) => {
                stream.queue.append(&mut queue);
                if should_wake {
                    let waker = stream.take_waker();
                    let has_waker = waker.0.is_some();
                    trace!(should_wake, has_waker, "slot::send_msg segment complete");
                    Ok(waker)
                } else {
                    trace!(
                        should_wake,
                        "slot::send_msg segment complete (no wakeup flag)"
                    );
                    Ok(half::AutoWake::default())
                }
            }
            None => Ok(half::AutoWake::default()),
        }
    }

    /// Set `binding_id` and open both receiver halves in one critical section.
    ///
    /// Acquires both half locks (stream → control) so that the binding store
    /// and the `HAS_RECEIVER` flag updates are never visible in a partial state.
    /// Returns `Ok(true)` if a new allocation was performed, `Ok(false)` if the
    /// slot was already bound (concurrent race), or `Err(Closed)` if the sender
    /// side has already been closed.
    #[inline]
    pub(crate) fn allocate_and_open(&self, binding_id: VarInt) -> Result<bool, half::Closed> {
        let mut s = self.stream.inner.lock();
        let mut c = self.control.inner.lock();

        // Re-check inside the lock: a concurrent caller may have already bound.
        if s.flags.contains(Flags::HAS_RECEIVER) && c.flags.contains(Flags::HAS_RECEIVER) {
            return Ok(false);
        }

        Self::allocate_and_open_locked(&mut s, &mut c, &self.binding_id, binding_id)?;
        // Clear any poisoned msg_table from the previous binding.
        s.extra.msg_table = None;
        Ok(true)
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

            // Clear any poisoned msg_table from the previous binding.
            s.extra.msg_table = None;

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

    /// Push Reset into both halves of an allocated slot without binding validation.
    ///
    /// Skips unallocated slots and halves without a receiver.  Does NOT clear
    /// `HAS_SENDER` — this is a transient notification (peer-dead cooldown),
    /// not a permanent close.
    pub(crate) fn broadcast_reset(&self, error_code: VarInt) -> (half::AutoWake, half::AutoWake) {
        let mut s = self.stream.inner.lock();
        let mut c = self.control.inner.lock();

        let raw = self.binding_id.load(Ordering::Relaxed);
        if raw & UNALLOCATED_BIT != 0 {
            return (half::AutoWake::default(), half::AutoWake::default());
        }

        // Poison the msg_table so in-flight chunk writes see Poisoned on complete.
        if let Some(table) = s.extra.msg_table.as_mut() {
            table.poison();
        }

        let sw = if s.flags.contains(Flags::HAS_RECEIVER) {
            s.queue
                .push_back(intrusive::Entry::new(msg::Stream::Reset { error_code }));
            s.take_waker()
        } else {
            half::AutoWake::default()
        };

        let cw = if c.flags.contains(Flags::HAS_RECEIVER) {
            c.queue
                .push_back(intrusive::Entry::new(msg::Control::Reset { error_code }));
            c.take_waker()
        } else {
            half::AutoWake::default()
        };

        (sw, cw)
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
        s: &mut HalfInner<msg::Stream, StreamState>,
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
fn validate_and_push<T, X>(
    binding_id: VarInt,
    entry: intrusive::Entry<T>,
    slot_binding: &AtomicU64,
    inner: &mut HalfInner<T, X>,
) -> Result<half::AutoWake, super::Error<intrusive::Entry<T>>> {
    if let Err(error) = validate_binding_state(binding_id, slot_binding, &inner.flags) {
        return Err(map_validation_error_entry(error, entry));
    }
    inner.queue.push_back(entry);
    Ok(inner.take_waker())
}

#[inline]
fn validate_msg_dispatch(
    binding_id: VarInt,
    slot_binding: &AtomicU64,
    inner: &HalfInner<msg::Stream, StreamState>,
) -> Result<(), super::Error<()>> {
    validate_binding_state(binding_id, slot_binding, &inner.flags).map_err(map_validation_error_unit)
}

/// Internal-only binding-state validation result used to centralize checks.
///
/// Callers must map this value to the appropriate `super::Error` form before
/// returning so public APIs continue to expose the existing error types.
#[derive(Clone, Copy, Debug)]
enum ValidationError {
    /// Incoming binding is from an older generation (or equal while unallocated).
    StaleBinding,
    /// Slot is unallocated, but incoming binding is newer than the last generation.
    Unallocated,
    /// Incoming binding is from a not-yet-active future generation.
    FutureBinding,
    /// Sender half has already been closed.
    SenderClosed,
    /// Receiver half has already been closed.
    HalfClosed,
}

/// Validates binding generation and half-open state for dispatch under lock.
#[inline]
fn validate_binding_state(
    binding_id: VarInt,
    slot_binding: &AtomicU64,
    flags: &Flags,
) -> Result<(), ValidationError> {
    let raw = slot_binding.load(Ordering::Relaxed);
    let stored = raw & !UNALLOCATED_BIT;
    let incoming = binding_id.as_u64();

    if raw & UNALLOCATED_BIT != 0 {
        if incoming <= stored {
            return Err(ValidationError::StaleBinding);
        }
        return Err(ValidationError::Unallocated);
    }
    if incoming < stored {
        return Err(ValidationError::StaleBinding);
    }
    if incoming > stored {
        return Err(ValidationError::FutureBinding);
    }
    if !flags.contains(Flags::HAS_SENDER) {
        return Err(ValidationError::SenderClosed);
    }
    if !flags.contains(Flags::HAS_RECEIVER) {
        return Err(ValidationError::HalfClosed);
    }
    Ok(())
}

#[inline]
fn map_validation_error_entry<T>(
    error: ValidationError,
    entry: intrusive::Entry<T>,
) -> super::Error<intrusive::Entry<T>> {
    match error {
        ValidationError::StaleBinding => super::Error::StaleBinding(entry),
        ValidationError::Unallocated => super::Error::Unallocated(entry),
        ValidationError::FutureBinding => super::Error::FutureBinding(entry),
        ValidationError::SenderClosed => super::Error::SenderClosed,
        ValidationError::HalfClosed => super::Error::HalfClosed(entry),
    }
}

#[inline]
fn map_validation_error_unit(error: ValidationError) -> super::Error<()> {
    match error {
        ValidationError::StaleBinding => super::Error::StaleBinding(()),
        ValidationError::Unallocated => super::Error::Unallocated(()),
        ValidationError::FutureBinding => super::Error::FutureBinding(()),
        ValidationError::SenderClosed => super::Error::SenderClosed,
        ValidationError::HalfClosed => super::Error::HalfClosed(()),
    }
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
    use core::sync::atomic::AtomicBool;

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
        assert!(slot
            .control
            .inner
            .lock()
            .flags
            .contains(Flags::HAS_RECEIVER));
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

    #[test]
    fn broadcast_reset_poisons_msg_table() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();

        // Insert a partial message (first chunk only of a 2-chunk message)
        let result = slot.push_msg(v(1), 0, 0, 16384, 8192, 0, 8192, false, true, |ptr, len| {
            unsafe { core::ptr::write_bytes(ptr, 0xAB, len as usize) };
            Ok::<(), ()>(())
        });
        assert!(result.is_ok());

        // Reset the slot
        slot.broadcast_reset(v(42));

        // The msg_table should be poisoned — second chunk write returns default waker
        let result = slot.push_msg(v(1), 0, 0, 16384, 8192, 1, 8192, false, true, |ptr, len| {
            unsafe { core::ptr::write_bytes(ptr, 0xCD, len as usize) };
            Ok::<(), ()>(())
        });
        // Should succeed (returns Ok with empty waker) — the chunk completes
        // but the poisoned table discards the result.
        assert!(result.is_ok());
    }

    #[test]
    fn new_binding_clears_msg_table() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();

        // Push a complete message to create and populate the msg_table
        let _ = slot.push_msg(v(1), 0, 0, 4096, 8192, 0, 4096, false, true, |ptr, len| {
            unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
            Ok::<(), ()>(())
        });

        // msg_table exists (even if entries were drained, the table itself is Some)
        assert!(slot.stream.inner.lock().extra.msg_table.is_some());

        // Simulate receiver drop + mark unallocated
        simulate_receiver_drop(&slot);

        // Re-bind with a new binding_id
        let result = slot.bind_and_push_stream(v(2), make_stream_entry());
        assert!(matches!(result, Ok(BindState::NewBinding(_))));

        // msg_table should be cleared — new sender starts fresh
        assert!(slot.stream.inner.lock().extra.msg_table.is_none());
    }

    #[test]
    fn push_msg_wakes_only_when_is_wakeup_set() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();

        // Register a waker on the stream half
        let (waker, wake_count) = test_waker();
        {
            let mut s = slot.stream.inner.lock();
            s.waker = Some(waker.clone());
        }

        // Push a complete message with is_wakeup=false
        let result = slot.push_msg(
            v(1),
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            false, // is_wakeup = false
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());
        // Waker should NOT have been taken
        assert_eq!(wake_count.load(Ordering::SeqCst), 0);

        // Re-register waker (it was consumed on the first push since we always take_waker
        // returns the slot — but with is_wakeup=false the AutoWake is default/empty)
        {
            let mut s = slot.stream.inner.lock();
            s.waker = Some(waker.clone());
        }

        // Push a complete message with is_wakeup=true
        let result = slot.push_msg(
            v(1),
            1,
            4096,
            4096,
            8192,
            0,
            4096,
            false,
            true, // is_wakeup = true
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());
        // Now the waker SHOULD fire
        drop(result);
        assert_eq!(wake_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn push_msg_detects_rebind_during_write_and_completes_to_new_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let msg_id = 0;
        let stream_offset = 0;
        let message_size = 4096;
        let chunk_size = 8192;
        let chunk_index = 0;
        let payload_len = 4096;
        let write_called = AtomicBool::new(false);

        let result = slot.push_msg(
            v(1),
            msg_id,
            stream_offset,
            message_size,
            chunk_size,
            chunk_index,
            payload_len,
            false,
            true,
            |ptr, len| {
                write_called.store(true, Ordering::Relaxed);
                simulate_receiver_drop(&slot);
                let result = slot.bind_and_push_stream(v(2), make_stream_entry());
                assert!(matches!(result, Ok(BindState::NewBinding(_))));
                unsafe { core::ptr::write_bytes(ptr, 0xAB, len as usize) };
                Ok::<(), ()>(())
            },
        );

        assert!(result.is_ok());
        assert!(write_called.load(Ordering::Relaxed));
        let mut stream = slot.stream.inner.lock();
        assert!(stream.extra.msg_table.is_none());
        assert_eq!(stream.queue.len(), 1);
        match stream.queue.pop_front().map(|e| e.into_inner()) {
            Some(msg::Stream::Data { payload, .. }) => {
                assert_eq!(payload.len(), 1);
                assert_eq!(payload[0], 42);
            }
            _ => panic!("expected rebound stream entry"),
        }
    }

    #[test]
    fn push_msg_detects_rebind_after_write_error() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();
        let write_called = AtomicBool::new(false);

        let result = slot.push_msg(
            v(1),
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            |_ptr, _len| {
                write_called.store(true, Ordering::Relaxed);
                simulate_receiver_drop(&slot);
                let result = slot.bind_and_push_stream(v(2), make_stream_entry());
                assert!(matches!(result, Ok(BindState::NewBinding(_))));
                Err::<(), ()>(())
            },
        );

        assert!(matches!(result, Err(super::super::MsgError::Write(()))));
        assert!(write_called.load(Ordering::Relaxed));
        let mut stream = slot.stream.inner.lock();
        assert!(stream.extra.msg_table.is_none());
        assert_eq!(stream.queue.len(), 1);
        match stream.queue.pop_front().map(|e| e.into_inner()) {
            Some(msg::Stream::Data { payload, .. }) => {
                assert_eq!(payload.len(), 1);
                assert_eq!(payload[0], 42);
            }
            _ => panic!("expected rebound stream entry"),
        }
    }

    #[test]
    fn push_msg_cancel_checkout_allows_retry() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1)).unwrap();

        let first = slot.push_msg(v(1), 0, 0, 4096, 8192, 0, 4096, false, true, |_ptr, _len| {
            Err::<(), ()>(())
        });
        assert!(matches!(first, Err(super::super::MsgError::Write(()))));

        let retry = slot.push_msg(v(1), 0, 0, 4096, 8192, 0, 4096, false, true, |ptr, len| {
            unsafe { core::ptr::write_bytes(ptr, 0xAA, len as usize) };
            Ok::<(), ()>(())
        });
        assert!(retry.is_ok());

        let mut stream = slot.stream.inner.lock();
        assert_eq!(stream.queue.len(), 1);
        match stream.queue.pop_front().map(|e| e.into_inner()) {
            Some(msg::Stream::Data { payload, .. }) => assert_eq!(payload.len(), 4096),
            _ => panic!("expected retried stream data"),
        }
    }
}
