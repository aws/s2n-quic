// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Pinned page-table of queue slots.
//!
//! Pages are `Pin<Box<[Slot]>>` so slot addresses are stable for the lifetime
//! of `PageTable`.  The page table grows by appending new pages; old pages are
//! never moved or reallocated.
//!
//! `SenderView` caches raw pointers into the pinned pages for O(1) slot lookup
//! on the dispatch hot path without holding the `RwLock`.

use super::slot::Slot;
use s2n_quic_core::varint::VarInt;
use std::{pin::Pin, sync::RwLock};

/// Minimum number of slots in the first page.
///
/// Subsequent pages double, so capacity grows as 1×, 2×, 4×, … of this value.
/// A smaller value in tests keeps allocation fast while still exercising growth.
pub(crate) const INITIAL_PAGE_SIZE: usize = if cfg!(test) { 8 } else { 1 << 16 };

// ── PageTable ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct PageTable {
    pub(crate) pages: RwLock<PageList>,
}

impl PageTable {
    pub(crate) fn new() -> Self {
        let mut list = PageList {
            pages: Vec::new(),
            total_slots: 0,
        };
        list.grow(INITIAL_PAGE_SIZE);
        Self {
            pages: RwLock::new(list),
        }
    }

    pub(crate) fn grow_to_fit(&self, index: usize) {
        let mut list = self.pages.write().unwrap();
        while list.total_slots <= index {
            let k = list.pages.len();
            let next_size = INITIAL_PAGE_SIZE << k;
            list.grow(next_size);
        }
    }

    pub(crate) fn total_slots(&self) -> usize {
        self.pages.read().unwrap().total_slots
    }
}

// ── PageList ────────────────────────────────────────────────────────────────

pub(crate) struct PageList {
    pages: Vec<Pin<Box<[Slot]>>>,
    pub(crate) total_slots: usize,
}

impl core::fmt::Debug for PageList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PageList")
            .field("page_count", &self.pages.len())
            .field("total_slots", &self.total_slots)
            .finish()
    }
}

impl PageList {
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
}

// ── SenderView (pointer cache) ──────────────────────────────────────────────

/// Per-worker pointer cache for O(1) slot lookup without holding the RwLock.
///
/// Refreshes lazily whenever a slot lookup falls outside the cached range
/// (i.e. on page growth, which is rare). The caller is responsible for
/// ensuring the `PageTable` outlives this view.
pub(crate) struct SenderView {
    cache: Vec<(*const Slot, usize)>,
    total_cached: usize,
}

// SAFETY: the raw pointers point into pinned pages that the caller guarantees
// outlive this view (typically via Arc on the owning state struct).
unsafe impl Send for SenderView {}
unsafe impl Sync for SenderView {}

impl SenderView {
    pub(crate) fn new() -> Self {
        Self {
            cache: Vec::new(),
            total_cached: 0,
        }
    }

    #[inline]
    pub(crate) fn get(&mut self, index: usize, pages: &PageTable) -> Option<&Slot> {
        if index >= self.total_cached {
            self.refresh(pages);
            if index >= self.total_cached {
                return None;
            }
        }
        let (page_idx, offset) = find_page(index);
        let (ptr, len) = self.cache.get(page_idx)?;
        if offset >= *len {
            return None;
        }
        // SAFETY: ptr is into a pinned allocation kept alive by the caller's
        // ownership of the PageTable (via Arc on the state struct).
        Some(unsafe { &*ptr.add(offset) })
    }

    #[inline]
    pub(crate) fn total_slots(&self) -> usize {
        self.total_cached
    }

    pub(crate) fn grow_to_fit(&mut self, index: usize, pages: &PageTable) {
        pages.grow_to_fit(index);
        self.refresh(pages);
    }

    pub(crate) fn for_each_slot(&mut self, pages: &PageTable, mut f: impl FnMut(&Slot)) {
        self.refresh(pages);
        for &(ptr, len) in &self.cache {
            for i in 0..len {
                f(unsafe { &*ptr.add(i) });
            }
        }
    }

    fn refresh(&mut self, pages: &PageTable) {
        let list = pages.pages.read().unwrap();
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
}

// ── Page index arithmetic ───────────────────────────────────────────────────

/// Map a flat slot index to `(page_index, intra-page offset)`.
///
/// Pages are sized P, 2P, 4P, 8P, …  where P = `INITIAL_PAGE_SIZE`.
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
            assert_eq!(find_page(i), (0, i));
        }
        // Page 1: indices [P, 3P)
        for i in 0..2 * INITIAL_PAGE_SIZE {
            assert_eq!(find_page(INITIAL_PAGE_SIZE + i), (1, i));
        }
    }

    #[test]
    fn sender_view_get() {
        let pages = PageTable::new();
        let mut view = SenderView::new();
        // First page has INITIAL_PAGE_SIZE slots
        assert!(view.get(0, &pages).is_some());
        assert!(view.get(INITIAL_PAGE_SIZE - 1, &pages).is_some());
        // Beyond first page — not yet grown
        assert!(view.get(INITIAL_PAGE_SIZE, &pages).is_none());
    }

    #[test]
    fn sender_view_grow() {
        let pages = PageTable::new();
        let mut view = SenderView::new();
        let target = INITIAL_PAGE_SIZE * 3;
        view.grow_to_fit(target, &pages);
        assert!(view.get(target, &pages).is_some());
    }
}
