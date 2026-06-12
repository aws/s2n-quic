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
use crate::{endpoint::msg, intrusive, tracing::*};
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
    /// The reader's currently-advertised receive window (`remote_max_data`),
    /// published lock-free by the reader so the dispatch side can bound its
    /// per-arrival credit release to what the reader actually acquired.
    ///
    /// The reader only debits the recv pool when it grows this window, but
    /// dispatch releases credit per arriving byte. A peer that overshoots the
    /// advertised window would otherwise make dispatch release credit that was
    /// never acquired — phantom credit that inflates the shared pool. Dispatch
    /// clamps each `observe_offset` to this ceiling so only acquired bytes are
    /// released.
    ///
    /// Written by the reader (`StreamReceiver::advertise_window`) with `Release`
    /// outside any half lock; read by dispatch under the stream-half lock with
    /// `Acquire`. Monotonic, and never below the unbacked initial window. A
    /// brief lag between the reader's store and a dispatch read only makes the
    /// clamp momentarily conservative, which is harmless for this bound.
    advertised_window: AtomicU64,
    pub(crate) stream: Half<msg::Stream, StreamState>,
    pub(crate) control: Half<msg::Control>,
}

pub(crate) struct StreamState {
    pub(crate) msg_table: Option<MsgTable>,
    /// Stream offset up to which MsgTable deliveries must always wake the reader.
    ///
    /// Set when a QueueData frame or a QueueMsg frame with `is_fin`/`is_wakeup`
    /// arrives. The value is the stream_offset of the frame that triggered the
    /// wakeup plus its payload length — i.e. the byte offset AFTER the waking
    /// frame. Any MsgTable message whose `stream_offset` falls below this
    /// watermark must fire the reader waker, because the reader was previously
    /// woken expecting contiguous data up to this point.
    ///
    /// Without this, a QueueData frame can wake the reader into a reassembler
    /// gap caused by a pending QueueMsg segment. The reader returns Pending.
    /// When the MsgTable segment later completes with `is_wakeup: false`, the
    /// reader is never re-woken and hangs indefinitely.
    pub(crate) flush_watermark: u64,
    /// Highest stream offset (exclusive end) ever observed for this binding,
    /// across QueueData and QueueMsg dispatches. Used to release recv-credit
    /// to the endpoint pool exactly once per byte regardless of retransmits
    /// or reordering.
    pub(crate) max_received_offset: u64,
    /// Bytes of unbacked initial window that have not yet been consumed by
    /// inbound data. Set to the configured initial window at slot bind; each
    /// new-byte release subtracts from this first, so only bytes beyond the
    /// initial window are returned to the recv credit pool.
    pub(crate) initial_window_remaining: u64,
    /// Set once the reader has reconciled its advertised window on termination
    /// (see [`StreamState::finish_recv_accounting`]). After this point
    /// [`StreamState::observe_offset`] stops releasing, because the terminal
    /// reconciliation already accounted for the full advertised window — any
    /// late-arriving (but in-window) bytes would otherwise be released twice.
    pub(crate) recv_finished: bool,
}

impl StreamState {
    pub(crate) fn clear(&mut self) {
        self.msg_table = None;
        self.flush_watermark = 0;
        self.max_received_offset = 0;
        self.recv_finished = false;
        // initial_window_remaining is reset by the caller on bind because the
        // value comes from configuration that lives outside the slot.
    }

    /// Update `max_received_offset` to cover `[offset, end)` and return how
    /// many of those bytes are pool-backed (i.e. beyond the initial window).
    /// `initial_window_remaining` is consumed monotonically for the new bytes.
    ///
    /// `advertised` is the reader's published receive window ceiling. `end` is
    /// clamped to it before accounting: the reader only ever acquired pool
    /// credit up to what it advertised, so bytes a (misbehaving) peer sends
    /// beyond that window must release nothing — releasing them would inject
    /// credit the pool never had. Clamping also keeps the invariant
    /// `max_received_offset <= advertised` that `finish_recv_accounting` relies
    /// on for an exact terminal reconciliation. The reader's own window
    /// enforcement still resets such a stream; this clamp only protects the
    /// shared pool's accounting in the meantime.
    ///
    /// Returns zero once [`finish_recv_accounting`] has run: the terminal
    /// reconciliation released the remainder of the advertised window in one
    /// shot, so releasing per-arrival again here would double-count.
    ///
    /// [`finish_recv_accounting`]: StreamState::finish_recv_accounting
    #[inline]
    pub(crate) fn observe_offset(&mut self, end: u64, advertised: u64) -> u64 {
        let end = end.min(advertised);
        if self.recv_finished || end <= self.max_received_offset {
            return 0;
        }
        let new_bytes = end - self.max_received_offset;
        self.max_received_offset = end;
        let from_initial = new_bytes.min(self.initial_window_remaining);
        self.initial_window_remaining -= from_initial;
        new_bytes - from_initial
    }

    /// Reconcile the advertised receive window on stream termination and return
    /// how many pool-backed credits the reader must release.
    ///
    /// A reader debits the recv pool whenever it grows its advertised window
    /// (`advertised`), but the dispatch side only releases credit for bytes that
    /// actually arrive (via [`observe_offset`]). The gap between the advertised
    /// window and what arrived — minus the still-unconsumed unbacked initial
    /// window, which was never acquired — is credit the reader holds but will
    /// never see filled. On termination that gap must go back to the pool:
    ///
    /// ```text
    /// leftover = advertised - max_received_offset - initial_window_remaining
    /// ```
    ///
    /// Window enforcement guarantees `max_received_offset <= advertised` and
    /// `max_received_offset + initial_window_remaining <= advertised`, so the
    /// result is exact; the saturating subtraction is defensive.
    ///
    /// Idempotent: the first call returns the leftover and latches
    /// `recv_finished`; subsequent calls (and any further `observe_offset`)
    /// return zero. Callers must hold the stream-half lock, which serializes
    /// this against concurrent dispatch `observe_offset` calls so a byte is
    /// released exactly once.
    ///
    /// [`observe_offset`]: StreamState::observe_offset
    #[inline]
    pub(crate) fn finish_recv_accounting(&mut self, advertised: u64) -> u64 {
        if self.recv_finished {
            return 0;
        }
        self.recv_finished = true;
        advertised
            .saturating_sub(self.max_received_offset)
            .saturating_sub(self.initial_window_remaining)
    }
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

/// Result of `Slot::allocate_and_open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllocateOutcome {
    /// A new binding was created and both receiver halves were opened.
    Allocated,
    /// The slot was already bound (a concurrent caller won the allocation).
    AlreadyBound,
    /// The incoming binding_id is `<=` the slot's tombstone — a stale/duplicate
    /// init for an already-recycled binding. Nothing was changed.
    Stale,
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
            advertised_window: AtomicU64::new(0),
            stream: Half::with_extra(StreamState {
                msg_table: None,
                flush_watermark: 0,
                max_received_offset: 0,
                initial_window_remaining: 0,
                recv_finished: false,
            }),
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

    /// Publish the reader's currently-advertised receive window so dispatch can
    /// clamp its per-arrival credit release to it. Lock-free; called by the
    /// reader off the stream-half lock. `fetch_max` keeps the published value
    /// monotonic against a stale concurrent store.
    #[inline]
    pub(crate) fn advertise_window(&self, advertised: u64) {
        self.advertised_window
            .fetch_max(advertised, Ordering::Release);
    }

    /// The reader's currently-advertised receive window, the ceiling dispatch
    /// uses to bound credit release. Read with `Acquire` to pair with
    /// [`Slot::advertise_window`]'s `Release`.
    #[inline]
    fn advertised_window(&self) -> u64 {
        self.advertised_window.load(Ordering::Acquire)
    }

    /// Reconcile the advertised receive window on termination, returning the
    /// pool-backed credits the reader must release. See
    /// [`StreamState::finish_recv_accounting`]. Idempotent; serialized with
    /// dispatch `observe_offset` by the stream-half lock.
    #[inline]
    pub(crate) fn finish_recv_accounting(&self, advertised: u64) -> u64 {
        self.stream
            .inner
            .lock()
            .extra
            .finish_recv_accounting(advertised)
    }

    /// Push to the stream half, validating binding_id inside the lock.
    ///
    /// On a successful Data push, returns the number of bytes whose offset
    /// extends past the highest observed end and that exceed the unbacked
    /// initial window — the caller releases that many credits to the recv
    /// pool. Duplicate or already-covered offsets contribute zero.
    #[inline]
    pub(crate) fn push_stream(
        &self,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<(half::AutoWake, u64), super::Error<intrusive::Entry<msg::Stream>>> {
        let mut inner = self.stream.inner.lock();
        if let Err(error) = validate_binding_state(binding_id, &self.binding_id, &inner.flags) {
            return Err(map_validation_error_entry(error, entry));
        }
        let mut release_bytes = 0u64;
        if let msg::Stream::Data {
            offset, payload, ..
        } = &*entry
        {
            let end = offset.as_u64().saturating_add(payload.len() as u64);
            if end > inner.extra.flush_watermark {
                inner.extra.flush_watermark = end;
            }
            release_bytes = inner.extra.observe_offset(end, self.advertised_window());
        }
        inner.queue.push_back(entry);
        Ok((inner.take_waker(), release_bytes))
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
    ///
    /// # Oversized-message deadlock breaker
    ///
    /// A QueueMsg message is delivered only once a whole segment is reassembled. A message whose
    /// extent (`stream_offset + message_size`) lies past the reader's advertised receive window can
    /// therefore *never* complete: every chunk stays checked out in the [`MsgTable`] and nothing is
    /// delivered. The writer's demand hints (`peer_max_offset` / the in-band `blocked` bit) ride
    /// inside those undelivered chunks, so the reader would never learn it must grow the window — a
    /// mutual stall (writer waits for window, reader waits for a completed message to learn the
    /// demand). We have the demand in hand here on the very first chunk, so when a segment won't fit
    /// the advertised window we inject a *synthetic* `msg::Stream::Blocked { synthetic: true }`
    /// carrying the writer's hinted demand directly onto the stream queue — bypassing the MsgTable,
    /// exactly as a real `QueueDataBlocked` would. The reader opens its window to that demand (see
    /// the "Window sizing" docs on [`crate::stream`]'s reader). The `synthetic` flag tells the reader
    /// this is known, bounded demand, NOT streaming back-pressure, so it must not ramp the
    /// speculative `growth_ratio` headroom. Deduped via a per-message latch on the MsgTable entry
    /// ([`MsgTable::mark_synth_signal_sent`]) so the signal goes out once per stalled message, not
    /// once per chunk.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_msg<E>(
        &self,
        binding_id: VarInt,
        msg_id: u64,
        stream_offset: u64,
        peer_max_offset: u64,
        message_size: u32,
        chunk_size: u16,
        chunk_index: u32,
        payload_len: u32,
        is_fin: bool,
        is_wakeup: bool,
        blocked: bool,
        write_fn: impl FnOnce(*mut u8, u32) -> Result<(), E>,
    ) -> Result<(half::AutoWake, u64), super::MsgError<E>> {
        // Validate + checkout under the stream lock.
        let (ptr, expected_len, chunk_index, keep_alive) = {
            let mut stream = self.stream.inner.lock();
            validate_msg_dispatch(binding_id, &self.binding_id, &stream)
                .map_err(super::MsgError::Queue)?;

            if is_fin || is_wakeup {
                let end = stream_offset.saturating_add(message_size as u64);
                if end > stream.extra.flush_watermark {
                    stream.extra.flush_watermark = end;
                }
            }

            let table = stream.extra.msg_table.get_or_insert_with(MsgTable::new);

            match table.insert(
                msg_id,
                stream_offset,
                peer_max_offset,
                message_size,
                chunk_size,
                chunk_index,
                payload_len,
                is_fin,
                blocked,
            ) {
                Ok(checkout) => (
                    checkout.ptr,
                    checkout.expected_len,
                    checkout.chunk_index,
                    checkout.keep_alive,
                ),
                Err(e) => {
                    trace!(msg_id, chunk_index, ?e, "slot::send_msg insert failed");
                    return Ok((half::AutoWake::default(), 0));
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
            if matches!(
                error,
                super::Error::HalfClosed(_) | super::Error::SenderClosed
            ) {
                if let Some(table) = stream.extra.msg_table.as_mut() {
                    table.cancel_checkout(msg_id, chunk_index);
                }
            }
            return Ok((half::AutoWake::default(), 0));
        }

        // The chunk wrote successfully and the binding is still valid: this
        // chunk's payload is now buffered. Charge it against the per-binding
        // dedup watermark and surface any new bytes for pool release.
        let chunk_start =
            stream_offset.saturating_add((chunk_index as u64).saturating_mul(chunk_size as u64));
        let chunk_end = chunk_start.saturating_add(payload_len as u64);
        let release_bytes = stream
            .extra
            .observe_offset(chunk_end, self.advertised_window());

        let local_queue = {
            let watermark = stream.extra.flush_watermark;
            let Some(table) = stream.extra.msg_table.as_mut() else {
                return Ok((half::AutoWake::default(), release_bytes));
            };
            match table.complete(msg_id, chunk_index) {
                super::msg_table::CompleteOutcome::Ready => {
                    let mut queue = intrusive::Queue::new();
                    let mut should_wake = false;
                    for delivered in table.drain_complete() {
                        should_wake |= delivered.stream_offset < watermark;
                        let entry: intrusive::Entry<msg::Stream> = msg::Stream::Data {
                            offset: VarInt::new(delivered.stream_offset).unwrap_or(VarInt::MAX),
                            peer_max_offset: VarInt::new(delivered.largest_offset)
                                .unwrap_or(VarInt::MAX),
                            fin: delivered.is_fin,
                            blocked: delivered.blocked,
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
                    Ok((waker, release_bytes))
                } else {
                    trace!(
                        should_wake,
                        "slot::send_msg segment complete (no wakeup flag)"
                    );
                    Ok((half::AutoWake::default(), release_bytes))
                }
            }
            // The segment did NOT complete this chunk. If its full extent lies past the reader's
            // advertised window it can *never* complete (every chunk stays checked out in the
            // MsgTable, nothing is delivered) and the writer's demand hints are stranded inside
            // those undelivered chunks — the synthetic signal is the ONLY path those hints can
            // reach the reader, since the chunks themselves never deliver. So carry the writer's
            // full hinted demand, not just this segment's end:
            //   * `peer_max_offset` — the writer's `largest_offset` high watermark (`next_offset +
            //     buffered_len`), i.e. everything it wants to send. Opening the window to this lets
            //     a multi-segment message advance without re-blocking once per segment.
            //   * `segment_end` (`stream_offset + message_size`) — the floor: this specific segment
            //     must be coverable to complete. `peer_max_offset` is normally >= it, but take the
            //     max defensively in case a hint lagged.
            // Deduped via a per-message latch on the MsgTable entry (`mark_synth_signal_sent`) so
            // the signal goes out once per stalled message, not once per chunk; a message's demand
            // is constant across its chunks, so there is no higher demand to re-signal. The returned
            // waker wakes a reader parked with no deliverable data.
            None => {
                let segment_end = stream_offset.saturating_add(message_size as u64);
                let desired = peer_max_offset.max(segment_end);
                let first_signal = desired > self.advertised_window()
                    && stream
                        .extra
                        .msg_table
                        .as_mut()
                        .is_some_and(|t| t.mark_synth_signal_sent(msg_id));
                if first_signal {
                    let entry: intrusive::Entry<msg::Stream> = msg::Stream::Blocked {
                        desired_offset: VarInt::new(desired).unwrap_or(VarInt::MAX),
                        synthetic: true,
                    }
                    .into();
                    stream.queue.push_back(entry);
                    trace!(
                        msg_id,
                        stream_offset,
                        message_size,
                        segment_end,
                        peer_max_offset,
                        desired,
                        advertised = self.advertised_window(),
                        "slot::send_msg synthetic blocked signal (segment exceeds advertised window)"
                    );
                    Ok((stream.take_waker(), release_bytes))
                } else {
                    Ok((half::AutoWake::default(), release_bytes))
                }
            }
        }
    }

    /// Set `binding_id` and open both receiver halves in one critical section.
    ///
    /// Acquires both half locks (stream → control) so that the binding store, the
    /// tombstone re-validation, and the `HAS_RECEIVER` flag updates are never
    /// visible in a partial state and never race a concurrent recycle. Returns:
    /// - `Ok(Allocated)` — a new binding was created and both halves opened.
    /// - `Ok(AlreadyBound)` — the slot was already bound (a concurrent caller won).
    /// - `Ok(Stale)` — the incoming `binding_id` is `<=` the slot's tombstone, i.e.
    ///   a stale/duplicate init for an already-recycled binding.
    /// - `Err(Closed)` — the sender side has already been closed.
    #[inline]
    pub(crate) fn allocate_and_open(
        &self,
        binding_id: VarInt,
        initial_window: u64,
    ) -> Result<AllocateOutcome, half::Closed> {
        let mut s = self.stream.inner.lock();
        let mut c = self.control.inner.lock();

        // Re-check inside the lock: a concurrent caller may have already bound.
        if s.flags.contains(Flags::HAS_RECEIVER) && c.flags.contains(Flags::HAS_RECEIVER) {
            return Ok(AllocateOutcome::AlreadyBound);
        }

        // Re-validate the binding_id against the slot's tombstone *under the lock*.
        // Callers (e.g. `ServerView::bind_for_msg`) classify the binding from a
        // lock-free `binding_id_raw()` read that is NOT atomic with this allocation:
        // between that read and acquiring these locks, a concurrent init could have
        // allocated this slot under a higher binding and then freed it, bumping the
        // tombstone. A recycled slot only accepts bindings strictly greater than its
        // previous binding_id, so reject anything `<=` the tombstone here — mirroring
        // `bind_and_push_stream`, which does the same comparison inside these locks.
        // Without this re-check a delayed/duplicate/reordered init could resurrect the
        // slot under a zombie binding_id below its own tombstone.
        let tombstone = self.binding_id.load(Ordering::Relaxed) & !UNALLOCATED_BIT;
        if binding_id.as_u64() <= tombstone {
            return Ok(AllocateOutcome::Stale);
        }

        Self::allocate_and_open_locked(&mut s, &mut c, &self.binding_id, binding_id)?;
        s.extra.clear();
        s.extra.initial_window_remaining = initial_window;
        // Seed the dispatch-side ceiling to the unbacked initial window; both
        // halves are locked, so this plain store can't race a reader.
        self.advertised_window
            .store(initial_window, Ordering::Release);
        Ok(AllocateOutcome::Allocated)
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
        initial_window: u64,
    ) -> Result<(BindState, u64), super::Error<intrusive::Entry<msg::Stream>>> {
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

            s.extra.clear();
            s.extra.initial_window_remaining = initial_window;
            // Seed the dispatch-side ceiling to the unbacked initial window the
            // reader starts with. A plain store (not fetch_max) resets any value
            // left by a prior generation on this recycled slot; both halves are
            // locked here so no reader for this binding can race the store.
            self.advertised_window
                .store(initial_window, Ordering::Release);

            let release_bytes = if let msg::Stream::Data {
                offset, payload, ..
            } = &*entry
            {
                let end = offset.as_u64().saturating_add(payload.len() as u64);
                if end > s.extra.flush_watermark {
                    s.extra.flush_watermark = end;
                }
                s.extra.observe_offset(end, self.advertised_window())
            } else {
                0
            };

            s.queue.push_back(entry);
            let waker = s.take_waker();
            return Ok((BindState::NewBinding(waker), release_bytes));
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

        let release_bytes = if let msg::Stream::Data {
            offset, payload, ..
        } = &*entry
        {
            let end = offset.as_u64().saturating_add(payload.len() as u64);
            if end > s.extra.flush_watermark {
                s.extra.flush_watermark = end;
            }
            s.extra.observe_offset(end, self.advertised_window())
        } else {
            0
        };

        s.queue.push_back(entry);
        let waker = s.take_waker();
        Ok((BindState::AlreadyBound(waker), release_bytes))
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
    validate_binding_state(binding_id, slot_binding, &inner.flags)
        .map_err(map_validation_error_unit)
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
        assert!(slot.allocate_and_open(v(1), 0).is_ok());
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
        assert!(slot.allocate_and_open(v(1), 0).is_err());
    }

    #[test]
    fn push_stream_matching_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();
        let result = slot.push_stream(v(1), make_stream_entry());
        assert!(result.is_ok());
    }

    #[test]
    fn push_stream_stale_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        let result = slot.push_stream(v(3), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn push_stream_future_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
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
        slot.allocate_and_open(v(1), 0).unwrap();
        let result = slot.push_control(v(1), make_control_entry());
        assert!(result.is_ok());
    }

    #[test]
    fn mark_unallocated_preserves_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        slot.mark_unallocated();
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_ne!(raw & UNALLOCATED_BIT, 0);
        assert_eq!(raw & !UNALLOCATED_BIT, 5);
    }

    #[test]
    fn mark_unallocated_rejects_old_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        slot.mark_unallocated();
        let result = slot.push_stream(v(5), make_stream_entry());
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_fresh_slot_new_binding() {
        let slot = Slot::with_queue_id(v(0));
        let result = slot.bind_and_push_stream(v(1), make_stream_entry(), 0);
        assert!(matches!(result, Ok((BindState::NewBinding(_), _))));
    }

    #[test]
    fn bind_and_push_already_bound_matching() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();
        let result = slot.bind_and_push_stream(v(1), make_stream_entry(), 0);
        assert!(matches!(result, Ok((BindState::AlreadyBound(_), _))));
    }

    #[test]
    fn bind_and_push_stale_on_allocated() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        let result = slot.bind_and_push_stream(v(3), make_stream_entry(), 0);
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_future_on_allocated() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        let result = slot.bind_and_push_stream(v(7), make_stream_entry(), 0);
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
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(4), make_stream_entry(), 0);
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_equal_on_recycled() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(5), make_stream_entry(), 0);
        assert!(matches!(result, Err(super::super::Error::StaleBinding(_))));
    }

    #[test]
    fn bind_and_push_new_binding_after_recycle() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot);
        let result = slot.bind_and_push_stream(v(6), make_stream_entry(), 0);
        assert!(matches!(result, Ok((BindState::NewBinding(_), _))));
    }

    // ── allocate_and_open tombstone re-validation (the `bind_for_msg` path) ──
    //
    // `ServerView::bind_for_msg` classifies the binding from a lock-free
    // `binding_id_raw()` read, then calls `allocate_and_open`. Those two steps are
    // not atomic: a concurrent init can allocate-and-recycle the slot in between,
    // bumping the tombstone. `allocate_and_open` must therefore re-check the
    // tombstone under the slot locks and reject a stale/equal binding — otherwise a
    // delayed/duplicate init resurrects the slot under a zombie binding_id below its
    // own tombstone. These tests drive `allocate_and_open` directly (the in-lock
    // check) on a recycled slot, the same coverage `bind_and_push_stream` has via
    // `bind_and_push_stale_on_recycled` / `bind_and_push_equal_on_recycled`.

    #[test]
    fn allocate_and_open_rejects_stale_on_recycled() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot); // tombstone = 5
                                       // A stale init (binding 4 < tombstone 5) must NOT resurrect the slot.
        assert_eq!(
            slot.allocate_and_open(v(4), 0),
            Ok(AllocateOutcome::Stale),
            "binding below the tombstone must be rejected under the lock"
        );
        // The slot stays unallocated with its tombstone intact.
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_ne!(raw & UNALLOCATED_BIT, 0, "slot must remain unallocated");
        assert_eq!(raw & !UNALLOCATED_BIT, 5, "tombstone must be unchanged");
    }

    #[test]
    fn allocate_and_open_rejects_equal_on_recycled() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot); // tombstone = 5
                                       // A duplicate init for the just-freed binding (5 == tombstone 5) is stale too.
        assert_eq!(
            slot.allocate_and_open(v(5), 0),
            Ok(AllocateOutcome::Stale),
            "binding equal to the tombstone must be rejected under the lock"
        );
    }

    #[test]
    fn allocate_and_open_allows_new_binding_after_recycle() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(5), 0).unwrap();
        simulate_receiver_drop(&slot); // tombstone = 5
                                       // A strictly-greater binding is a legitimate new generation.
        assert_eq!(
            slot.allocate_and_open(v(6), 0),
            Ok(AllocateOutcome::Allocated),
            "binding above the tombstone must allocate"
        );
        let raw = slot.binding_id.load(Ordering::Relaxed);
        assert_eq!(raw & UNALLOCATED_BIT, 0, "slot must now be allocated");
        assert_eq!(raw & !UNALLOCATED_BIT, 6);
    }

    #[test]
    fn bind_and_push_after_broadcast_close() {
        let slot = Slot::with_queue_id(v(0));
        slot.broadcast_close();
        let result = slot.bind_and_push_stream(v(1), make_stream_entry(), 0);
        assert!(matches!(result, Err(super::super::Error::SenderClosed)));
    }

    #[test]
    fn bind_and_push_half_closed() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();
        // Close stream receiver
        slot.stream.inner.lock().flags.remove(Flags::HAS_RECEIVER);
        let result = slot.bind_and_push_stream(v(1), make_stream_entry(), 0);
        assert!(matches!(result, Err(super::super::Error::HalfClosed(_))));
    }

    #[test]
    fn broadcast_close_both_halves() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();
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
        slot.allocate_and_open(v(1), 0).unwrap();

        // Insert a partial message (first chunk only of a 2-chunk message)
        let result = slot.push_msg(
            v(1),
            0,
            0,
            0,
            16384,
            8192,
            0,
            8192,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0xAB, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());

        // Reset the slot
        slot.broadcast_reset(v(42));

        // The msg_table should be poisoned — second chunk write returns default waker
        let result = slot.push_msg(
            v(1),
            0,
            0,
            0,
            16384,
            8192,
            1,
            8192,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0xCD, len as usize) };
                Ok::<(), ()>(())
            },
        );
        // Should succeed (returns Ok with empty waker) — the chunk completes
        // but the poisoned table discards the result.
        assert!(result.is_ok());
    }

    #[test]
    fn new_binding_clears_msg_table() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();

        // Push a complete message to create and populate the msg_table
        let _ = slot.push_msg(
            v(1),
            0,
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );

        // msg_table exists (even if entries were drained, the table itself is Some)
        assert!(slot.stream.inner.lock().extra.msg_table.is_some());

        // Simulate receiver drop + mark unallocated
        simulate_receiver_drop(&slot);

        // Re-bind with a new binding_id
        let result = slot.bind_and_push_stream(v(2), make_stream_entry(), 0);
        assert!(matches!(result, Ok((BindState::NewBinding(_), _))));

        // msg_table should be cleared — new sender starts fresh
        assert!(slot.stream.inner.lock().extra.msg_table.is_none());
    }

    #[test]
    fn push_msg_wakes_based_on_flush_watermark() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();

        // Register a waker on the stream half
        let (waker, wake_count) = test_waker();
        {
            let mut s = slot.stream.inner.lock();
            s.waker = Some(waker.clone());
        }

        // Push a complete message with is_wakeup=false — watermark stays at 0,
        // so stream_offset(0) is NOT < watermark(0) → no wake.
        let result = slot.push_msg(
            v(1),
            0,
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            false,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(wake_count.load(Ordering::SeqCst), 0);

        // Re-register waker
        {
            let mut s = slot.stream.inner.lock();
            s.waker = Some(waker.clone());
        }

        // Push a complete message with is_wakeup=true — watermark advances to
        // stream_offset(4096) + message_size(4096) = 8192. The delivered message
        // at stream_offset(4096) < watermark(8192) → wakes.
        let result = slot.push_msg(
            v(1),
            1,
            4096,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());
        drop(result);
        assert_eq!(wake_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn push_msg_detects_rebind_during_write_and_completes_to_new_binding() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();
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
            0,
            message_size,
            chunk_size,
            chunk_index,
            payload_len,
            false,
            true,
            false,
            |ptr, len| {
                write_called.store(true, Ordering::Relaxed);
                simulate_receiver_drop(&slot);
                let result = slot.bind_and_push_stream(v(2), make_stream_entry(), 0);
                assert!(matches!(result, Ok((BindState::NewBinding(_), _))));
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
        slot.allocate_and_open(v(1), 0).unwrap();
        let write_called = AtomicBool::new(false);

        let result = slot.push_msg(
            v(1),
            0,
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            false,
            |_ptr, _len| {
                write_called.store(true, Ordering::Relaxed);
                simulate_receiver_drop(&slot);
                let result = slot.bind_and_push_stream(v(2), make_stream_entry(), 0);
                assert!(matches!(result, Ok((BindState::NewBinding(_), _))));
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
        slot.allocate_and_open(v(1), 0).unwrap();

        let first = slot.push_msg(
            v(1),
            0,
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            false,
            |_ptr, _len| Err::<(), ()>(()),
        );
        assert!(matches!(first, Err(super::super::MsgError::Write(()))));

        let retry = slot.push_msg(
            v(1),
            0,
            0,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0xAA, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(retry.is_ok());

        let mut stream = slot.stream.inner.lock();
        assert_eq!(stream.queue.len(), 1);
        match stream.queue.pop_front().map(|e| e.into_inner()) {
            Some(msg::Stream::Data { payload, .. }) => assert_eq!(payload.len(), 4096),
            _ => panic!("expected retried stream data"),
        }
    }

    /// After slot recycling via `bind_and_push_stream`, the flush_watermark from
    /// the previous binding must be cleared. A stale watermark causes spurious
    /// wakeups on the new binding's QueueMsg deliveries even when the sender
    /// explicitly set `is_wakeup: false`.
    ///
    /// Scenario:
    /// 1. First binding sets a high flush_watermark via is_wakeup=true message
    /// 2. Receiver drops (slot recycled via mark_unallocated)
    /// 3. New binding arrives via bind_and_push_stream
    /// 4. New sender sends QueueMsg with is_wakeup=false
    /// 5. BUG: the stale watermark causes should_wake=true (spurious wake)
    #[test]
    fn bind_and_push_stream_clears_flush_watermark() {
        let slot = Slot::with_queue_id(v(0));
        slot.allocate_and_open(v(1), 0).unwrap();

        // First binding: push a message with is_wakeup=true to set watermark high.
        // stream_offset=0, message_size=65536 → watermark = 65536
        let result = slot.push_msg(
            v(1),
            0,
            0,
            0,
            65536,
            8192,
            0,
            8192,
            false,
            true,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());

        // Verify watermark was set
        assert_eq!(slot.stream.inner.lock().extra.flush_watermark, 65536);

        // Simulate receiver drop + recycling
        simulate_receiver_drop(&slot);

        // New binding arrives via bind_and_push_stream
        let result = slot.bind_and_push_stream(v(2), make_stream_entry(), 0);
        assert!(matches!(result, Ok((BindState::NewBinding(_), _))));

        // The stale watermark from the prior binding must be cleared. The
        // first data entry of the new binding can advance it to its own end
        // offset — `make_stream_entry` is a 1-byte payload at offset 0, so
        // the watermark resets and is bumped to 1, well below the 65536 the
        // prior binding left behind.
        assert_eq!(
            slot.stream.inner.lock().extra.flush_watermark,
            1,
            "BUG: flush_watermark was not cleared after bind_and_push_stream. \
             A stale watermark from the previous binding causes spurious wakeups \
             on the new binding's QueueMsg deliveries, violating is_wakeup=false semantics."
        );

        // Drain the initial entry pushed by bind_and_push_stream
        slot.stream.inner.lock().queue.pop_front();

        // Register a waker to verify wakeup behavior
        let (waker, wake_count) = test_waker();
        slot.stream.inner.lock().waker = Some(waker);

        // New binding: push a complete message at offset 4096 with is_wakeup=false.
        // The watermark (1, set by the bind's data push) is below the message's
        // stream_offset, so should_wake=false and the waker should NOT fire —
        // the stale 65536 watermark from the previous binding would have caused
        // a spurious wake.
        let result = slot.push_msg(
            v(2),
            0,
            4096,
            0,
            4096,
            8192,
            0,
            4096,
            false,
            false,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok::<(), ()>(())
            },
        );
        assert!(result.is_ok());
        drop(result);

        assert_eq!(
            wake_count.load(Ordering::SeqCst),
            0,
            "BUG: reader was spuriously woken despite is_wakeup=false. \
             The stale flush_watermark from the previous binding caused \
             should_wake to evaluate as true for the new binding's messages."
        );
    }

    /// Regression (review finding M1): `observe_offset` must clamp its per-arrival recv-credit
    /// release to the reader's *advertised* receive window. The reader only acquires pool credit up
    /// to what it advertises (growing `remote_max_data`), but dispatch releases per arriving byte;
    /// without the clamp a peer that overshoots the advertised window makes dispatch release credit
    /// that was never acquired, silently inflating the shared pool and defeating backpressure.
    ///
    /// The bootstrap case is the most reachable: a slot binds with `initial_window_remaining = 0`
    /// (or the unbacked window already consumed), so every arriving byte is pool-backed — yet the
    /// reader may have advertised only a small window.
    #[test]
    fn observe_offset_clamps_release_to_advertised_window() {
        let mut state = StreamState {
            msg_table: None,
            flush_watermark: 0,
            max_received_offset: 0,
            // No unbacked initial window: model the post-bootstrap / zero-initial-window slot where
            // every byte is accounted as pool-backed.
            initial_window_remaining: 0,
            recv_finished: false,
        };

        // The reader advertised (and therefore acquired) only 10 bytes of pool credit. A peer that
        // overshoots to 1000 bytes must release at most the advertised 10; the excess releases
        // nothing because the reader never acquired it.
        let advertised = 10u64;
        let released = state.observe_offset(1000, advertised);
        assert_eq!(
            released, advertised,
            "observe_offset must clamp its release to the advertised window ({advertised}), \
             got {released}",
        );

        // A second overshooting arrival must release nothing further: the ceiling is already
        // fully accounted, so no phantom credit can be injected.
        let released_again = state.observe_offset(2000, advertised);
        assert_eq!(
            released_again, 0,
            "no further credit may be released once the advertised window is fully observed",
        );
    }

    /// Within the advertised window `observe_offset` still releases exactly the pool-backed bytes
    /// (those beyond the unbacked initial window), so the clamp does not under-release for a
    /// well-behaved peer.
    #[test]
    fn observe_offset_releases_pool_backed_bytes_within_window() {
        let mut state = StreamState {
            msg_table: None,
            flush_watermark: 0,
            max_received_offset: 0,
            initial_window_remaining: 4,
            recv_finished: false,
        };

        // Advertised window comfortably above what arrives. First 4 bytes are unbacked (release 0),
        // the next 6 are pool-backed (release 6).
        let advertised = 100u64;
        assert_eq!(state.observe_offset(4, advertised), 0);
        assert_eq!(state.observe_offset(10, advertised), 6);
    }
}
