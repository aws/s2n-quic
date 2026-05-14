// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Hierarchical Timing Wheel for scheduling.
//!
//! The wheel receives entries from an inner receiver, inserts them into the
//! appropriate time slot based on their target time, and yields batches
//! of due entries as a `Receiver<List<A>>`.
//!
//! The wheel uses 256 slots per level with a configurable base tick (default 1µs).
//! 4 levels cover 256^4 ticks ≈ 4.29 billion µs ≈ 71.6 minutes at 1µs granularity.
//!
//! Each level maintains a 256-bit occupancy bitset ([u64; 4]) for O(1) next-expiry
//! computation via `trailing_zeros`. Causal ordering is preserved: entries from the
//! same sender are always dequeued in the order they were submitted, because the
//! inner receiver and per-slot lists are FIFO.

use crate::{
    clock::precision,
    intrusive_queue::{Adapter, List},
    socket::channel,
};
use core::{
    fmt,
    task::{self, Poll},
};

#[cfg(test)]
mod tests;

// ── Configuration ──────────────────────────────────────────────────────────

/// Number of slots per level (must be a power of 2).
const SLOTS_PER_LEVEL: usize = 256;

/// Bit-shift per level (log2 of SLOTS_PER_LEVEL).
const BITS_PER_LEVEL: u32 = 8; // 2^8 = 256

/// Mask for extracting slot index within a level.
const SLOT_MASK: u64 = (SLOTS_PER_LEVEL - 1) as u64;

/// Number of u64 words in the occupancy bitset per level.
/// 256 bits = 4 × u64.
const BITSET_WORDS: usize = SLOTS_PER_LEVEL / 64;

/// Number of hierarchical levels.
/// With 256 slots/level at 1µs base: covers 256^4 µs ≈ 71.6 minutes.
const LEVELS: usize = 4;

/// Default base tick granularity in microseconds.
pub const DEFAULT_GRANULARITY_US: u64 = 1;

// ── Conversion utilities ───────────────────────────────────────────────────

/// Convert a tick count to a timestamp given a granularity.
#[inline]
pub const fn tick_to_timestamp(tick: u64, granularity_us: u64) -> precision::Timestamp {
    let time = std::time::Duration::from_micros(tick * granularity_us);
    precision::Timestamp {
        nanos: time.as_nanos() as _,
    }
}

/// Convert a timestamp to a tick count given a granularity.
#[inline]
pub const fn timestamp_to_tick(timestamp: precision::Timestamp, granularity_us: u64) -> u64 {
    let time = std::time::Duration::from_nanos(timestamp.nanos);
    time.as_micros() as u64 / granularity_us
}

// ── WheelAdapter trait ─────────────────────────────────────────────────────

/// Extension trait for adapters that can be used with the timing wheel.
///
/// This trait provides per-link timing information, allowing different links
/// in the same value to have different target times.
pub trait WheelAdapter: Adapter {
    /// Returns the target time for this link, or `None` for immediate execution.
    ///
    /// # Safety
    /// The pointer must be valid and point to an initialized Value.
    unsafe fn target_time(value: *const Self::Value) -> Option<precision::Timestamp>;

    /// Sets the actual execution time (called when the entry is yielded from the wheel).
    ///
    /// # Safety
    /// The pointer must be valid and point to an initialized Value.
    unsafe fn set_target_time(value: *mut Self::Value, time: precision::Timestamp);
}

/// Trait for types that have a single target time for wheel scheduling.
///
/// This is a simpler interface than `WheelAdapter` for types that only need
/// a single timer per value.
pub trait SingleTimer {
    /// Returns the target time, or `None` for immediate execution.
    fn target_time(&self) -> Option<precision::Timestamp>;

    /// Sets the target time (called when the entry is yielded from the wheel).
    fn set_target_time(&mut self, time: precision::Timestamp);
}

// Implement WheelAdapter for EntryAdapter<T> where T: SingleTimer
impl<T: SingleTimer> WheelAdapter for crate::intrusive_queue::EntryAdapter<T> {
    unsafe fn target_time(value: *const Self::Value) -> Option<precision::Timestamp> {
        (*Self::target(value as *mut Self::Value)).target_time()
    }

    unsafe fn set_target_time(value: *mut Self::Value, time: precision::Timestamp) {
        (*Self::target(value)).set_target_time(time);
    }
}

// ── Wheel (async receiver) ─────────────────────────────────────────────────

/// Hierarchical timing wheel that implements `Receiver<List<A>>`.
///
/// The wheel receives batches of entries from an inner `Receiver<List<A>>`,
/// inserts them into time slots based on their target time, and yields
/// batches of due entries. It manages its own timer to wake when entries are due.
pub struct Wheel<A, Timer, R, const GRANULARITY_US: u64 = DEFAULT_GRANULARITY_US>
where
    A: Adapter,
{
    inner: R,
    timer: Timer,
    levels: Box<[Level<A>; LEVELS]>,
    current_tick: u64,
    pending_list: List<A>,
    len: usize,
}

impl<A, Timer, R, const GRANULARITY_US: u64> fmt::Debug for Wheel<A, Timer, R, GRANULARITY_US>
where
    A: Adapter,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wheel")
            .field("current_tick", &self.current_tick)
            .field("granularity_us", &GRANULARITY_US)
            .field("levels", &LEVELS)
            .field("slots_per_level", &SLOTS_PER_LEVEL)
            .finish()
    }
}

impl<A, Timer, R, const GRANULARITY_US: u64> Wheel<A, Timer, R, GRANULARITY_US>
where
    A: WheelAdapter,
    Timer: precision::Timer,
    R: channel::Receiver<List<A>>,
{
    /// Create a new wheel with an inner receiver and timer.
    pub fn new(inner: R, timer: Timer) -> Self {
        let initial_tick = Self::timestamp_to_tick(timer.now());
        Self {
            inner,
            timer,
            levels: Box::new(core::array::from_fn(|_| Level::new())),
            current_tick: initial_tick,
            pending_list: List::new(),
            len: 0,
        }
    }

    // ── Internal: Tick/Timestamp arithmetic (convenience wrappers) ──────

    #[inline]
    fn tick_to_timestamp(tick: u64) -> precision::Timestamp {
        tick_to_timestamp(tick, GRANULARITY_US)
    }

    #[inline]
    fn timestamp_to_tick(timestamp: precision::Timestamp) -> u64 {
        timestamp_to_tick(timestamp, GRANULARITY_US)
    }

    // ── Internal: Wheel operations ──────────────────────────────────────

    /// Insert a single entry into the appropriate level/slot.
    fn insert_entry(&mut self, ptr: A::Pointer) {
        let target_tick = unsafe {
            // SAFETY: The pointer is valid since it came from the adapter.
            let value_ptr = A::as_ptr(&ptr);
            A::target_time(value_ptr)
        }
        .map(Self::timestamp_to_tick)
        .unwrap_or(self.current_tick)
        .max(self.current_tick);

        let (level, slot_idx) = Self::compute_level_and_slot(self.current_tick, target_tick);

        self.levels[level].push_back(slot_idx, ptr);
        self.len += 1;
    }

    /// Advance the virtual clock to `target_tick` and return all due entries.
    fn tick_to(&mut self, now: precision::Timestamp) -> List<A> {
        let target_tick = Self::timestamp_to_tick(now);
        let target_tick = target_tick.max(self.current_tick);

        // Fast path: if wheel is empty, just advance time without scanning slots
        if self.len == 0 {
            self.current_tick = target_tick;
            return List::new();
        }

        let mut result = List::new();

        while self.current_tick < target_tick {
            let current_slot = (self.current_tick & SLOT_MASK) as usize;

            // Use the bitset to find the next occupied slot in level 0.
            let next_occupied_tick = self.levels[0]
                .first_occupied_after(current_slot)
                .map(|slot| (self.current_tick & !SLOT_MASK) | slot as u64);

            // The advance limit is the sooner of target_tick or the next
            // 256-aligned cascade boundary.
            let next_cascade = (self.current_tick | SLOT_MASK) + 1;
            let advance_limit = target_tick.min(next_cascade);

            match next_occupied_tick {
                Some(occ_tick) if occ_tick < advance_limit => {
                    // Jump to the occupied slot, drain it
                    let occ_slot = (occ_tick & SLOT_MASK) as usize;
                    self.current_tick = occ_tick;
                    let mut slot_queue = self.levels[0].drain(occ_slot);
                    self.len -= slot_queue.len();
                    result.append(&mut slot_queue);
                    self.current_tick += 1;
                }
                _ => {
                    // Nothing to drain before the advance limit — skip ahead
                    self.current_tick = advance_limit;
                }
            }

            // Cascade when we land on a 256-aligned boundary
            if self.current_tick & SLOT_MASK == 0 {
                self.cascade(self.current_tick, 1);
            }
        }

        // Also drain the current slot (entries at exactly target_tick)
        let slot_idx = (self.current_tick & SLOT_MASK) as usize;
        let mut slot_queue = self.levels[0].drain(slot_idx);
        self.len -= slot_queue.len();
        result.append(&mut slot_queue);

        result
    }

    /// Returns the timestamp of the earliest non-empty slot, if any.
    fn next_expiry(&self) -> Option<precision::Timestamp> {
        let base = self.current_tick;

        for level in 0..LEVELS {
            let shift = BITS_PER_LEVEL * level as u32;
            let current_level_slot = (base >> shift) & SLOT_MASK;
            let cursor = ((current_level_slot + 1) & SLOT_MASK) as usize;

            let hit = self.levels[level].first_occupied_after(cursor).or_else(|| {
                if cursor > 0 {
                    self.levels[level].first_occupied_after(0)
                } else {
                    None
                }
            });

            if let Some(slot_idx) = hit {
                let slot_offset = if slot_idx as u64 > current_level_slot {
                    slot_idx as u64 - current_level_slot
                } else {
                    SLOTS_PER_LEVEL as u64 - current_level_slot + slot_idx as u64
                };

                let base_aligned = (base >> shift) << shift;
                let earliest_tick = base_aligned + (slot_offset << shift);
                return Some(Self::tick_to_timestamp(earliest_tick.max(base + 1)));
            }
        }

        None
    }

    /// Cascade entries from level `level` down toward level 0.
    fn cascade(&mut self, current_tick: u64, mut level: usize) {
        while level < LEVELS {
            let slot_idx = ((current_tick >> (BITS_PER_LEVEL * level as u32)) & SLOT_MASK) as usize;

            let mut entries = self.levels[level].drain(slot_idx);

            // Iterate in reverse to preserve FIFO order
            while let Some(ptr) = entries.pop_back() {
                let target_tick = unsafe {
                    // SAFETY: The pointer is valid since it came from the adapter.
                    let value_ptr = A::as_ptr(&ptr);
                    A::target_time(value_ptr)
                }
                .map(Self::timestamp_to_tick)
                .unwrap_or(current_tick)
                .max(current_tick);

                let (new_level, new_slot) = Self::compute_level_and_slot(current_tick, target_tick);
                self.levels[new_level].push_front(new_slot, ptr);
            }

            // Check if this level also wrapped and needs to cascade upward
            let next_level_tick = current_tick >> (BITS_PER_LEVEL * level as u32);
            if next_level_tick & SLOT_MASK == 0 {
                level += 1;
                continue;
            }

            break;
        }
    }

    /// Compute which level and slot an entry should be placed in.
    fn compute_level_and_slot(current_tick: u64, target_tick: u64) -> (usize, usize) {
        let delta = target_tick - current_tick;

        if delta == 0 {
            let slot = (current_tick & SLOT_MASK) as usize;
            return (0, slot);
        }

        let mut level = 0;
        let mut shifted = delta;
        while shifted >= SLOTS_PER_LEVEL as u64 && level + 1 < LEVELS {
            shifted >>= BITS_PER_LEVEL;
            level += 1;
        }

        let slot = ((target_tick >> (BITS_PER_LEVEL * level as u32)) & SLOT_MASK) as usize;

        (level, slot)
    }

    /// Drain all remaining entries at current time (called when inner is closed).
    fn drain_remaining(&mut self) -> List<A> {
        let mut result = List::new();
        for level in &mut *self.levels {
            for slot_idx in 0..SLOTS_PER_LEVEL {
                let mut queue = level.drain(slot_idx);
                result.append(&mut queue);
            }
        }
        self.len = 0;
        result
    }
}

impl<A, Timer, R, const GRANULARITY_US: u64> channel::Receiver<List<A>>
    for Wheel<A, Timer, R, GRANULARITY_US>
where
    A: WheelAdapter,
    Timer: precision::Timer,
    R: channel::Receiver<List<A>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut channel::Budget,
    ) -> Poll<Option<List<A>>> {
        // 1. Try to drain one batch from inner receiver into the wheel
        match self.inner.poll_recv(cx, budget) {
            Poll::Ready(Some(batch)) => {
                // Sort entries: immediate execution goes to pending, scheduled goes to wheel
                for ptr in batch {
                    let has_target_time = unsafe {
                        // SAFETY: The pointer is valid since it came from the adapter.
                        let value_ptr = A::as_ptr(&ptr);
                        A::target_time(value_ptr).is_some()
                    };

                    if !has_target_time {
                        // No timestamp = immediate execution, skip wheel insertion
                        self.pending_list.push_back(ptr);
                    } else {
                        self.insert_entry(ptr);
                    }
                }
                // Signal more work available
                budget.set_needs_wake();
            }
            Poll::Ready(None) => {
                // Inner is closed - drain remaining entries and close
                let mut final_list = core::mem::take(&mut self.pending_list);
                let mut remaining = self.drain_remaining();
                final_list.append(&mut remaining);

                return Poll::Ready(if final_list.is_empty() {
                    None
                } else {
                    Some(final_list)
                });
            }
            Poll::Pending => {
                // No new batches, continue to check if entries are due
            }
        }

        // 2. Tick to current time and collect due entries
        let now = self.timer.now();
        let mut list = self.tick_to(now);
        self.pending_list.append(&mut list);

        // 3. If we have entries, return them
        if !self.pending_list.is_empty() {
            return Poll::Ready(Some(core::mem::take(&mut self.pending_list)));
        }

        // 4. No entries due yet - update timer for next expiry
        if let Some(expiry) = self.next_expiry() {
            self.timer.update(expiry);
        } else {
            // No entries in wheel, cancel timer and wait for inner receiver
            self.timer.cancel();
            return Poll::Pending;
        }

        // 5. Poll timer - if Ready, signal needs_wake to tick again; if Pending, we're done
        match self.timer.poll_ready(cx) {
            Poll::Ready(()) => {
                // Timer fired, signal more work to tick again
                budget.set_needs_wake();
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Level ──────────────────────────────────────────────────────────────────

/// One level of the hierarchical wheel.
struct Level<A: Adapter> {
    slots: [List<A>; SLOTS_PER_LEVEL],
    occupied: [u64; BITSET_WORDS],
}

impl<A: Adapter> Level<A> {
    fn new() -> Self {
        let slots = core::array::from_fn(|_| List::new());
        Self {
            slots,
            occupied: [0; BITSET_WORDS],
        }
    }

    #[inline]
    fn set_occupied(&mut self, index: usize) {
        let word = index / 64;
        let bit = index % 64;
        self.occupied[word] |= 1u64 << bit;
    }

    #[inline]
    fn clear_occupied(&mut self, index: usize) {
        let word = index / 64;
        let bit = index % 64;
        self.occupied[word] &= !(1u64 << bit);
    }

    #[inline]
    fn push_back(&mut self, index: usize, ptr: A::Pointer) {
        debug_assert!(index < SLOTS_PER_LEVEL);
        unsafe { self.slots.get_unchecked_mut(index) }.push_back(ptr);
        self.set_occupied(index);
    }

    #[inline]
    fn push_front(&mut self, index: usize, ptr: A::Pointer) {
        debug_assert!(index < SLOTS_PER_LEVEL);
        unsafe { self.slots.get_unchecked_mut(index) }.push_front(ptr);
        self.set_occupied(index);
    }

    #[inline]
    fn drain(&mut self, index: usize) -> List<A> {
        debug_assert!(index < SLOTS_PER_LEVEL);
        let queue = core::mem::take(unsafe { self.slots.get_unchecked_mut(index) });
        self.clear_occupied(index);
        queue
    }

    fn first_occupied_after(&self, from_slot: usize) -> Option<usize> {
        debug_assert!(from_slot < SLOTS_PER_LEVEL);

        let start_word = from_slot / 64;
        let start_bit = from_slot % 64;

        let masked = self.occupied[start_word] & (!0u64 << start_bit);
        if masked != 0 {
            return Some(start_word * 64 + masked.trailing_zeros() as usize);
        }

        for w in (start_word + 1)..BITSET_WORDS {
            if self.occupied[w] != 0 {
                return Some(w * 64 + self.occupied[w].trailing_zeros() as usize);
            }
        }

        None
    }
}
