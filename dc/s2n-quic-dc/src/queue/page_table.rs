// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Pinned page-table of queue slots.
//!
//! Pages are `Pin<Box<[Slot]>>` so slot addresses are stable for the lifetime
//! of `State`.  The page table grows by appending new pages; old pages are
//! never moved or reallocated.
//!
//! `SenderView` caches raw pointers into the pinned pages for O(1) slot lookup
//! on the dispatch hot path without holding the `RwLock`.

use super::slot::Slot;
use s2n_quic_core::varint::VarInt;
use std::{
    pin::Pin,
    ptr::NonNull,
    sync::{Arc, RwLock},
};

/// Minimum number of slots in the first page.
///
/// Subsequent pages double, so capacity grows as 1×, 2×, 4×, … of this value.
/// A smaller value in tests keeps allocation fast while still exercising growth.
pub(crate) const INITIAL_PAGE_SIZE: usize = if cfg!(test) { 8 } else { 1 << 16 };

// ── Shared state ─────────────────────────────────────────────────────────────

pub(crate) struct PageTable {
    pub(crate) state: Arc<State>,
}

impl PageTable {
    pub(crate) fn new() -> Self {
        let state = Arc::new(State {
            pages: RwLock::new(PageList {
                pages: Vec::new(),
                total_slots: 0,
            }),
        });
        // Pre-allocate the first page so there are always slots available.
        {
            let mut list = state.pages.write().unwrap();
            list.grow(INITIAL_PAGE_SIZE);
        }
        PageTable { state }
    }

    /// Allocate a new `SenderView` backed by this page table.
    pub(crate) fn sender_view(&self) -> SenderView {
        SenderView {
            state: self.state.clone(),
            cache: Vec::new(),
            total_cached: 0,
        }
    }

    /// Grow the table until it can hold `index`.
    ///
    /// Pages are sized P, 2P, 4P, 8P, … where P = `INITIAL_PAGE_SIZE`, so
    /// page `k` has `2^k × P` slots and `find_page` indexes into them in O(1).
    pub(crate) fn grow_to_fit(&self, index: usize) {
        let mut list = self.state.pages.write().unwrap();
        while list.total_slots <= index {
            // Page k (0-based) has 2^k × P slots.
            let k = list.pages.len();
            let next_size = INITIAL_PAGE_SIZE << k;
            list.grow(next_size);
        }
    }

    pub(crate) fn total_slots(&self) -> usize {
        self.state.pages.read().unwrap().total_slots
    }
}

impl Clone for PageTable {
    fn clone(&self) -> Self {
        PageTable {
            state: self.state.clone(),
        }
    }
}

pub(crate) struct State {
    pub(crate) pages: RwLock<PageList>,
}

pub(crate) struct PageList {
    /// Each entry is a pinned, heap-allocated array of slots.
    pages: Vec<Pin<Box<[Slot]>>>,
    pub(crate) total_slots: usize,
}

impl PageList {
    /// Append a new page of `page_size` fresh slots starting at `base_index`.
    fn grow(&mut self, page_size: usize) {
        let base_index = self.total_slots;
        let slots: Vec<Slot> = (0..page_size)
            .map(|i| {
                let id = VarInt::new((base_index + i) as u64).unwrap_or(VarInt::MAX);
                Slot::with_queue_id(id)
            })
            .collect();
        let boxed: Box<[Slot]> = slots.into_boxed_slice();
        let pinned = Pin::new(boxed);
        self.total_slots += page_size;
        self.pages.push(pinned);
    }

    /// Return a raw pointer to the slot at absolute index `index`, or `None`
    /// if `index` is out of range.
    ///
    /// SAFETY: the returned pointer is valid for as long as `State` (and thus
    /// the pinned allocation) is kept alive.
    #[allow(dead_code)]
    fn slot_ptr(&self, index: usize) -> Option<NonNull<Slot>> {
        if index >= self.total_slots {
            return None;
        }
        let (page_idx, offset) = find_page(index);
        let page = self.pages.get(page_idx)?;
        let ptr = &page[offset] as *const Slot as *mut Slot;
        Some(unsafe { NonNull::new_unchecked(ptr) })
    }
}

// ── Dispatch hot-path view ────────────────────────────────────────────────────

/// Per-thread (or per-dispatch-worker) view of the page table.
///
/// Caches raw pointers for O(1) slot lookup without holding the `RwLock`.
/// Refreshes its cache lazily whenever a slot lookup falls outside the cached
/// range (i.e. on page growth, which is rare).
#[derive(Clone)]
pub(crate) struct SenderView {
    state: Arc<State>,
    /// Cached (base_ptr, page_len) per page, in page order.
    cache: Vec<(*const Slot, usize)>,
    /// Total number of slots covered by `cache`.
    total_cached: usize,
}

// SAFETY: the raw pointers point into pinned pages owned by `Arc<State>` which
// is cloned into this struct, keeping the pages alive.  Slots are never moved.
unsafe impl Send for SenderView {}
unsafe impl Sync for SenderView {}

impl SenderView {
    /// Look up the slot at absolute index `index`.
    ///
    /// Returns `None` if the index is out of range.
    #[inline]
    pub(crate) fn get(&mut self, index: usize) -> Option<&Slot> {
        if index >= self.total_cached {
            self.refresh();
            if index >= self.total_cached {
                return None;
            }
        }
        let (page_idx, offset) = find_page(index);
        let (ptr, len) = self.cache.get(page_idx)?;
        if offset >= *len {
            return None;
        }
        // SAFETY: ptr is into a pinned allocation kept alive by Arc<State>.
        Some(unsafe { &*ptr.add(offset) })
    }

    /// Total number of slots currently visible in the cache.
    #[inline]
    pub(crate) fn total_slots(&self) -> usize {
        self.total_cached
    }

    /// Access the underlying shared state (for passing to receivers).
    #[inline]
    pub(crate) fn state(&self) -> &Arc<State> {
        &self.state
    }

    /// Grow the page table until it can hold `index`.
    pub(crate) fn grow_to_fit(&mut self, index: usize) {
        {
            let mut list = self.state.pages.write().unwrap();
            while list.total_slots <= index {
                let k = list.pages.len();
                let next_size = INITIAL_PAGE_SIZE << k;
                list.grow(next_size);
            }
        }
        self.refresh();
    }

    /// Refresh the cache from the shared `RwLock<PageList>`.
    ///
    /// Called at most once per page growth — infrequent.
    fn refresh(&mut self) {
        let list = self.state.pages.read().unwrap();
        if list.total_slots <= self.total_cached {
            return;
        }
        self.cache.clear();
        self.total_cached = 0;
        for page in &list.pages {
            let ptr = page.as_ptr();
            let len = page.len();
            self.cache.push((ptr, len));
            self.total_cached += len;
        }
    }

    /// Iterate all currently allocated slots (used by broadcast-close).
    ///
    /// Refreshes the cache first so newly grown pages are included.
    pub(crate) fn for_each_slot(&mut self, mut f: impl FnMut(&Slot)) {
        self.refresh();
        for &(ptr, len) in &self.cache {
            for i in 0..len {
                // SAFETY: same as `get`.
                f(unsafe { &*ptr.add(i) });
            }
        }
    }
}

// ── Page index arithmetic ────────────────────────────────────────────────────

/// Map a flat slot index to `(page_index, intra-page offset)`.
///
/// Pages are sized P, 2P, 4P, 8P, …  where P = `INITIAL_PAGE_SIZE` (the first
/// page is pre-allocated with P slots and each subsequent grow doubles).
///
/// - Page 0 covers `[0, P)` → 1×P slots
/// - Page 1 covers `[P, 3P)` → 2×P slots
/// - Page 2 covers `[3P, 7P)` → 4×P slots
///
/// The formula: let `m = index / P`; page_idx = ⌊log₂(m+1)⌋;
/// page_start = P × (2^page_idx − 1).
#[inline]
pub(crate) fn find_page(index: usize) -> (usize, usize) {
    let m = index / INITIAL_PAGE_SIZE;
    let page_idx = (m + 1).ilog2() as usize;
    let page_start = ((1usize << page_idx) - 1) * INITIAL_PAGE_SIZE;
    (page_idx, index - page_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_page_basic() {
        // Page 0: indices [0, P)
        for i in 0..INITIAL_PAGE_SIZE {
            let (page, offset) = find_page(i);
            assert_eq!(page, 0, "index {i}");
            assert_eq!(offset, i, "index {i}");
        }
        // Page 1: indices [P, 3P) — 2P slots
        let (page, offset) = find_page(INITIAL_PAGE_SIZE);
        assert_eq!(page, 1);
        assert_eq!(offset, 0);

        let (page, offset) = find_page(2 * INITIAL_PAGE_SIZE - 1);
        assert_eq!(page, 1);
        assert_eq!(offset, INITIAL_PAGE_SIZE - 1);
    }

    #[test]
    fn find_page_2p_boundary() {
        // Index 2P is the start of page 1's second half, still page 1
        let (page, offset) = find_page(2 * INITIAL_PAGE_SIZE);
        assert_eq!(page, 1, "2P should still be page 1");
        assert_eq!(offset, INITIAL_PAGE_SIZE);

        // Index 3P is the start of page 2
        let (page, offset) = find_page(3 * INITIAL_PAGE_SIZE);
        assert_eq!(page, 2, "3P should start page 2");
        assert_eq!(offset, 0);
    }

    #[test]
    fn page_table_grow_and_lookup() {
        let pt = PageTable::new();
        let initial = pt.total_slots();
        assert_eq!(initial, INITIAL_PAGE_SIZE);

        // After one grow: total_slots = P + 2P = 3P
        pt.grow_to_fit(INITIAL_PAGE_SIZE);
        assert_eq!(pt.total_slots(), 3 * INITIAL_PAGE_SIZE);

        let mut view = pt.sender_view();
        // slot 0 should be accessible
        assert!(view.get(0).is_some());
        // slot at 2P-1 (end of page 1) should be accessible
        assert!(view.get(2 * INITIAL_PAGE_SIZE - 1).is_some());
        // slot at 2P (page 1, second half) should be accessible
        assert!(view.get(2 * INITIAL_PAGE_SIZE).is_some());
        // 3P - 1 is the last slot in page 1
        assert!(view.get(3 * INITIAL_PAGE_SIZE - 1).is_some());
    }

    #[test]
    fn slot_queue_ids_are_correct() {
        let pt = PageTable::new();
        pt.grow_to_fit(3 * INITIAL_PAGE_SIZE);

        let mut view = pt.sender_view();
        for i in 0..pt.total_slots() {
            let slot = view.get(i).expect("slot should exist");
            let qid = slot.queue_id();
            assert_eq!(qid.as_u64() as usize, i, "slot at {i} should have queue_id {i}");
        }
    }

    #[test]
    fn sender_view_refresh_on_growth() {
        let pt = PageTable::new();
        let mut view = pt.sender_view();
        // Access within initial range works
        assert!(view.get(0).is_some());
        // Beyond initial range is None
        assert!(view.get(INITIAL_PAGE_SIZE).is_none());
        // Grow the table
        pt.grow_to_fit(INITIAL_PAGE_SIZE);
        // View auto-refreshes on cache miss
        assert!(view.get(INITIAL_PAGE_SIZE).is_some());
    }

    #[test]
    fn sender_view_out_of_range_none() {
        let pt = PageTable::new();
        let mut view = pt.sender_view();
        assert!(view.get(INITIAL_PAGE_SIZE * 100).is_none());
    }

    #[test]
    fn for_each_slot_visits_all() {
        let pt = PageTable::new();
        pt.grow_to_fit(3 * INITIAL_PAGE_SIZE);
        let mut view = pt.sender_view();
        let mut count = 0;
        view.for_each_slot(|_| count += 1);
        assert_eq!(count, pt.total_slots());
    }
}
