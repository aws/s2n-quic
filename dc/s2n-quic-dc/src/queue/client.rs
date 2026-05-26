// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Client-side queue allocation and dispatch.
//!
//! ## Roles
//!
//! - `ClientAllocator` — stream creation path.  Allocates a local page-table
//!   slot and a `dest_queue_id` from the peer's `FreeList`, then opens the
//!   receiver handles.  Each `ClientAllocator` owns a dedicated `SenderView`
//!   so repeated allocations avoid re-acquiring the page-table `RwLock`.
//!
//! - `ClientDispatch` — inbound packet dispatch path.  Routes an incoming
//!   `msg::Stream` / `msg::Control` entry to the correct slot by `queue_id`
//!   after validating `binding_id`.  No allocation logic here.
//!
//! - `ClientFreeList` — local slot recycling.  Uses a `HierarchicalBitSet`
//!   that starts small and grows on demand, plus a high-water mark for fresh
//!   slot bump allocation.

use super::{
    half::AutoWake,
    handle::{AllocResult, ControlReceiver, OnFree, StreamReceiver},
    page_table::{PageTable, SenderView},
    Error,
};
use crate::{bitset, endpoint::msg, intrusive, sync};
use s2n_quic_core::varint::VarInt;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

// ── ClientFreeList ────────────────────────────────────────────────────────────

/// Local slot recycling for the client page table.
///
/// Freed slot indices are stored in a `HierarchicalBitSet` for O(4) pop.
/// The bitset starts with capacity 1 and grows on demand so that fresh
/// allocators pay no up-front memory cost.
/// Fresh slots beyond the current high-water mark are allocated by bumping.
pub struct ClientFreeList {
    freed: bitset::HierarchicalBitSet,
    high_water_mark: usize,
    closed: bool,
}

impl ClientFreeList {
    pub(crate) fn new() -> Self {
        Self {
            freed: bitset::HierarchicalBitSet::new(1),
            high_water_mark: 0,
            closed: false,
        }
    }

    /// Pop the next available local slot index.
    ///
    /// Returns `None` if the free list is closed.
    /// Prefers recycled indices (pop from bitset) over fresh ones (bump).
    pub(crate) fn pop(&mut self) -> Option<usize> {
        if self.closed {
            return None;
        }
        if let Some(idx) = self.freed.pop_first() {
            return Some(idx as usize);
        }
        let idx = self.high_water_mark;
        self.high_water_mark += 1;
        Some(idx)
    }

    /// Return a freed slot index back to the recycling set.
    pub(crate) fn push_freed(&mut self, index: usize) {
        let idx = index as u32;
        if idx >= self.freed.capacity() {
            if idx >= bitset::HierarchicalBitSet::MAX_CAPACITY {
                return;
            }
            self.freed.grow(idx + 1);
        }
        let newly_inserted = self.freed.insert(idx);
        debug_assert!(newly_inserted, "double-free of slot {index}");
    }

    pub(crate) fn close(&mut self) {
        self.closed = true;
    }
}

// ── LocalState ────────────────────────────────────────────────────────────────

/// Shared mutable state for client-side allocation: slot recycling + binding counter.
pub(crate) struct LocalState {
    /// Monotonically increasing binding_id generator.  Starts at 1 so that
    /// fresh slots (whose stored binding starts at 0) always accept the first bind.
    next_binding: AtomicU64,
    /// Local slot index recycling.
    pub(crate) free: Mutex<ClientFreeList>,
}

impl LocalState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            next_binding: AtomicU64::new(1),
            free: Mutex::new(ClientFreeList::new()),
        })
    }

    fn next_binding_id(&self) -> VarInt {
        let id = self.next_binding.fetch_add(1, Ordering::Relaxed);
        VarInt::new(id).expect("binding_id overflow")
    }
}

// ── ClientAllocator ───────────────────────────────────────────────────────────

/// Allocates local queue slots and peer `dest_queue_id`s for client streams.
///
/// Each `ClientAllocator` has its own `SenderView`, so repeated calls to
/// `try_alloc` amortise the page-table `RwLock` acquisition — the view is
/// only refreshed on page growth.
#[derive(Clone)]
pub struct ClientAllocator {
    view: SenderView,
    local: Arc<LocalState>,
    peer_free: Arc<sync::free_list::FreeList>,
}

impl ClientAllocator {
    pub fn new(peer_free: Arc<sync::free_list::FreeList>) -> Self {
        let page_table = PageTable::new();
        let view = page_table.sender_view();
        Self {
            view,
            local: LocalState::new(),
            peer_free,
        }
    }

    pub fn try_alloc(&mut self) -> Option<AllocResult> {
        let dest_queue_id = self.peer_free.try_alloc()?;
        self.alloc_local(dest_queue_id).ok()
    }

    pub fn peer_free(&self) -> &Arc<sync::free_list::FreeList> {
        &self.peer_free
    }

    pub fn close(&self) {
        self.local.free.lock().unwrap().close();
    }

    fn alloc_local(&mut self, dest_queue_id: VarInt) -> Result<AllocResult, Error<()>> {
        let index = {
            let mut free = self.local.free.lock().unwrap();
            match free.pop() {
                Some(idx) => idx,
                None => return Err(Error::SenderClosed),
            }
        };

        if index >= self.view.total_slots() {
            self.view.grow_to_fit(index);
        }

        let slot_ref = self
            .view
            .get(index)
            .expect("slot index out of range after grow");

        let binding_id = self.local.next_binding_id();

        if slot_ref.allocate_and_open(binding_id).is_err() {
            self.local.free.lock().unwrap().push_freed(index);
            return Err(Error::SenderClosed);
        }

        let slot_ptr = slot_ref.as_ptr();
        let local_queue_id = VarInt::new(index as u64).expect("slot index exceeds VarInt range");
        let on_free = OnFree::Client {
            _state: self.view.state().clone(),
            local_free: self.local.clone(),
        };

        Ok(AllocResult {
            stream: StreamReceiver::new(slot_ptr, on_free.clone()),
            control: ControlReceiver::new(slot_ptr, on_free),
            local_queue_id,
            dest_queue_id,
            binding_id,
        })
    }

    /// Build a `ClientDispatch` backed by the same page table.
    pub fn dispatcher(&self) -> ClientDispatch {
        ClientDispatch {
            view: self.view.clone(),
        }
    }
}

// ── ClientDispatch ────────────────────────────────────────────────────────────

/// Routes inbound packets to allocated client slots.
///
/// `queue_id` is the local slot index (what the peer sent as `dest_queue_id`).
/// `binding_id` is validated against the slot's stored value before pushing.
pub struct ClientDispatch {
    view: SenderView,
}

impl ClientDispatch {
    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_stream(binding_id, entry)
    }

    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Control>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Control>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_control(binding_id, entry)
    }

    /// Broadcast-close all currently allocated slots.
    ///
    /// Called when the path secret entry is evicted.  `AutoWake` tokens are
    /// passed to `waker_sink` — the caller can `.take()` to batch wakers for
    /// later, or simply drop the token to wake immediately.
    pub fn close(&mut self, waker_sink: &mut impl FnMut(AutoWake)) {
        self.view.for_each_slot(|slot| {
            let (sw, cw) = slot.broadcast_close();
            waker_sink(sw);
            waker_sink(cw);
        });
    }
}
