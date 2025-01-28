// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Slab allocator drawing from a fixed arena.
//!
//! The arena is allocated at initialization time, providing a fixed memory region from which to
//! allocate entries from. We support allocating a compile-time fixed set of types, however, the
//! internals are mostly uncaring about *what* that set is (including the size).
//!
//! The arena has three types of pages:
//!
//! * Free (empty)
//! * Partially allocated
//! * Fully initialized/allocated
//!
//! Initially, all pages start empty. When an allocation request is made, a page is moved into the
//! partially allocated state. A u16 counter is placed at the top of the page for the # of entries
//! allocated so far,. A given size class always allocates from this page until the page is
//! exhausted. A page, when moved into the partially allocated state, is also threaded into an
//! intrusive doubly linked list of allocated pages for this size class. This list supports
//! deallocation operations. A partially-empty page, if it exists, is always at the top of this
//! list.
//!
//! Effectively, we have a `LinkedList<Vec<T>>` for each T, with at most one of the Vecs being
//! non-fixed-size.
//!
//! On deallocation, we swap the entry we just allocated with one from the top of the page list.
//! This ensures that at most one page for this type is not contiguously allocated, meaning that
//! wasted memory due to fragmentation is bounded to a single page per allocatable type.

#![allow(dead_code)]

use std::alloc::Layout;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ptr::NonNull;
use std::sync::Mutex;

#[derive(Debug)]
pub struct Allocator {
    inner: Mutex<AllocatorInner>,
}

#[derive(Debug)]
struct AllocatorInner {
    // layout and region are only used for Drop, otherwise we always manage through the other
    // fields.
    layout: Layout,
    region: NonNull<u8>,

    free_pages: Vec<NonNull<u8>>,

    // This slab indirects into the allocator's internally memory, allowing us to move allocated
    // entries ~transparently to callers.
    //
    // FIXME: Remove the indirection by moving the containing memory (path secret entries) into the
    // allocator and/or guarantee that they don't move without invalidation of the child via `Pin`.
    parents: slab::Slab<parking_lot::Mutex<Option<u32>>>,

    // These are lists of *allocated* entries.
    allocated_pages: BTreeMap<BySize, VecDeque<NonNull<u8>>>,

    // For each size class, the page we are currently allocating from.
    //
    // When this runs out we can grab a new region from `free_pages`.
    reserved_page: HashMap<Layout, Option<NonNull<u8>>>,
}

#[derive(Debug, PartialEq, Eq)]
struct BySize(Layout);

impl std::ops::Deref for BySize {
    type Target = Layout;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Ord for BySize {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .size()
            .cmp(&other.0.size())
            .then(self.0.align().cmp(&other.0.align()))
    }
}

impl PartialOrd for BySize {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// Note: must contain at least one entry for every size class.
// 2**14 is sufficient for our purposes.
const PAGE_SIZE: usize = 1 << 13;

impl Allocator {
    pub fn with_capacity(capacity: usize) -> Allocator {
        let layout =
            Layout::from_size_align(capacity.next_multiple_of(PAGE_SIZE), PAGE_SIZE).unwrap();
        let region = unsafe { NonNull::new(std::alloc::alloc(layout)).unwrap() };
        // ensures step_by does the right thing.
        assert_eq!(layout.size() % PAGE_SIZE, 0);
        let free_pages: Vec<_> = (0..layout.size())
            .step_by(PAGE_SIZE)
            .map(|offset| unsafe { NonNull::new(region.as_ptr().add(offset)).unwrap() })
            .collect();
        let end = (region.as_ptr() as usize)
            .checked_add(layout.size())
            .unwrap();
        for page in free_pages.iter().copied() {
            let start = (page.as_ptr() as usize).checked_add(PAGE_SIZE).unwrap();
            assert!(start <= end, "{:x} < {:x}", start, end);

            let end = page.as_ptr().wrapping_add(PAGE_SIZE & !(PAGE_SIZE - 1));
            assert_eq!(
                page.as_ptr() as usize,
                (end as usize - 1) & !(PAGE_SIZE - 1)
            );
        }
        let inner = AllocatorInner {
            layout,
            region,
            parents: slab::Slab::new(),
            free_pages,
            allocated_pages: Default::default(),
            reserved_page: Default::default(),
        };
        Allocator {
            inner: Mutex::new(inner),
        }
    }

    /// Allocate `Layout`.
    ///
    /// Returns a handle which can be used to lookup the allocation.
    pub fn allocate(&self, layout: Layout) -> AllocationGuard<'_> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let handle = inner.allocate(layout);
        // this allocation cannot be freed yet as we didn't release the `inner` lock.
        inner.read_allocation(self, handle).unwrap()
    }

    pub fn read_allocation(&self, handle: usize) -> Option<AllocationGuard<'_>> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let guard = inner.parents[handle].lock();
        let Some(offset) = *guard else {
            return None;
        };

        parking_lot::MutexGuard::leak(guard);

        Some(AllocationGuard {
            this: self,
            mutex: handle,
            ptr: unsafe {
                NonNull::new(inner.region.as_ptr().add(usize::try_from(offset).unwrap())).unwrap()
            },
        })
    }

    /// Must only be called once.
    pub unsafe fn deallocate(&self, handle: usize) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        let entry = inner.parents.remove(handle);
        // FIXME: ABA avoidance needed?
        assert!(!entry.is_locked());
        let Some(offset) = entry.into_inner() else {
            // If already deallocated, nothing more to do.
            return;
        };
        let deallocate_ptr = unsafe { inner.region.as_ptr().add(usize::try_from(offset).unwrap()) };
        // Round the pointer we are deallocating down to the page size, giving us a pointer to the
        // start of the page.
        let deallocating_page = map_addr(deallocate_ptr, |addr| addr & !(PAGE_SIZE - 1));
        let layout = deallocating_page
            .add(std::mem::size_of::<u16>())
            .cast::<Layout>()
            .read_unaligned();

        // Lookup `layout` (after adjustment) in reserved_page, if possible, popping the last
        // allocated entry, and potentially freeing the page. If not possible, that means
        // reserved_page is empty;
        if let Some(Some(start)) = inner.reserved_page.get_mut(&layout) {
            unsafe {
                let page_start = start.as_ptr();

                let mut page = PageWithLayout {
                    start: page_start,
                    layout,
                };

                page.pop_to(&mut inner, deallocate_ptr);

                if page.count() == 0 {
                    // remove this reserved page.
                    assert!(inner.reserved_page.remove(&layout).is_some());

                    // we already popped the page, so just put it back on the free pages list.
                    inner.free_pages.push(NonNull::new(page_start).unwrap());
                }

                return;
            }
        }

        // Lookup `layout` in allocated pages and pop an entry off of a page, moving that page into
        // `reserved` and replacing the deallocated entry with the popped one.
        //
        // Note that no reserved page exists (since we didn't exit above).
        if let Some(pages) = inner.allocated_pages.get_mut(&BySize(layout)) {
            unsafe {
                let page_start = pages.pop_back().unwrap().as_ptr();

                let mut page = PageWithLayout {
                    start: page_start,
                    layout,
                };

                page.pop_to(&mut inner, deallocate_ptr);

                // This is reachable if this is the only entry on the page.
                if page.count() == 0 {
                    // we already popped the page, so just put it back on the free pages list.
                    inner.free_pages.push(NonNull::new(page_start).unwrap());
                }

                // OK, at this point `deallocated` is filled, and we have a page with N-1 entries
                // that we add to reserved_page.
                inner
                    .reserved_page
                    .insert(layout, Some(NonNull::new(page_start).unwrap()));

                return;
            }
        }

        // This entry cannot be in a partial page pre-dealloc (it would be a reserved page in that
        // case) and it cannot be a full page (2nd case was hit above). No other cases exist.
        unreachable!()
    }
}

struct PageWithLayout {
    start: *mut u8,
    layout: Layout,
}

impl PageWithLayout {
    fn count(&self) -> u16 {
        unsafe { self.start.cast::<u16>().read() }
    }

    fn add_count(&mut self, v: i16) {
        unsafe {
            *self.start.cast::<u16>() = self.count().checked_add_signed(v).unwrap();
        }
    }

    /// Pop an entry from this page and move the contents into `dest`.
    ///
    /// `dest` is assumed to be an entry (possibly on this page) which is of size `layout`.
    unsafe fn pop_to(&mut self, inner: &mut AllocatorInner, dest: *mut u8) {
        let page_end = self.start.add(PAGE_SIZE & !(self.layout.align() - 1));

        let last_allocated = page_end.sub(self.layout.size() * self.count() as usize);
        // last_allocated.is_aligned_to(layout.align()), except stable.
        assert!(last_allocated as usize & (self.layout.align() - 1) == 0);

        self.add_count(-1);

        // If we are deallocating the last entry on this page, no parent swapping is
        // needed, we can just drop it.
        if last_allocated != dest {
            // Lock the parent pointer for the entry we're popping and replacing the
            // deallocated entry with.
            let src_parent_idx = last_allocated.cast::<u32>().read() as usize;
            let mut src_parent = inner.parents[src_parent_idx].lock();
            assert!(src_parent.is_some());

            // Copy the data into the now-deallocated entry.
            dest.copy_from_nonoverlapping(last_allocated, self.layout.size());

            // Update parent pointer to point to new entry.
            *src_parent =
                Some(u32::try_from(dest as usize - inner.region.as_ptr() as usize).unwrap());
            drop(src_parent);
        }
    }

    /// Add an entry to this page.
    ///
    /// Returns Some(entry address) if this was successful.
    fn push(&mut self) -> Option<*mut u8> {
        unsafe {
            let page_end = self.start.add(PAGE_SIZE & !(self.layout.align() - 1));

            let last_allocated = page_end.sub(self.layout.size() * self.count() as usize);
            // last_allocated.is_aligned_to(layout.align()), except stable.
            assert!(last_allocated as usize & (self.layout.align() - 1) == 0);

            let new_allocation = last_allocated.wrapping_sub(self.layout.size());

            if new_allocation
                >= self
                    .start
                    .add(std::mem::size_of::<u16>())
                    .add(std::mem::size_of::<Layout>())
            {
                self.add_count(1);
                Some(new_allocation)
            } else {
                None
            }
        }
    }
}

pub struct AllocationGuard<'a> {
    this: &'a Allocator,
    mutex: usize,
    ptr: NonNull<u8>,
}

impl AllocationGuard<'_> {
    pub fn as_ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    pub fn handle(&self) -> usize {
        self.mutex
    }
}

impl Drop for AllocationGuard<'_> {
    fn drop(&mut self) {
        // creation leaked the guard, so now we force unlock.
        unsafe {
            self.this
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .parents[self.mutex]
                .force_unlock();
        }
    }
}

impl AllocatorInner {
    fn allocate(&mut self, layout: Layout) -> usize {
        // Add parent pointer field.
        let (layout, _) = Layout::new::<u32>().extend(layout).unwrap();
        let layout = layout.pad_to_align();

        let reserved = self.reserved_page.entry(layout).or_insert_with(|| None);
        let align = layout.align();
        assert!(align.is_power_of_two());

        let allocation = 'allocate: loop {
            match reserved {
                Some(page_start) => {
                    let mut page = PageWithLayout {
                        start: page_start.as_ptr(),
                        layout,
                    };
                    if let Some(ptr) = page.push() {
                        break ptr;
                    } else {
                        // move before the counter
                        self.allocated_pages
                            .entry(BySize(layout))
                            .or_default()
                            .push_back(*page_start);
                        // no more entries left...
                        *reserved = None;
                        // fallthrough
                    }
                }
                None => {}
            }

            // Ok, we failed to pull from the reserved page, re-populate reserved.
            if let Some(page) = self.free_pages.pop() {
                unsafe {
                    // Each page has a u16 counter at the front of allocated entries.
                    // Initialize the counter.
                    // It is discoverable later by aligning the end of the page down.
                    page.as_ptr().cast::<u16>().write(0);
                    page.as_ptr()
                        .add(std::mem::size_of::<u16>())
                        .cast::<Layout>()
                        .write_unaligned(layout);
                    *reserved = Some(page);

                    // and loop around to allocate from the reserved page...
                    continue;
                }
            }

            // Ok, no free pages left either, we need to deallocate entries.
            for (page_layout, pages) in self.allocated_pages.iter_mut().rev() {
                let Some(page) = pages.pop_front() else {
                    continue;
                };

                // OK, we are going to empty this page and return it to free pages, which will then
                // move it into the reserved pages for `layout`.
                //
                // We need to deallocate all of the entries on this page (in theory we can try to
                // move them into a reserved page if it's available, but for simplicitly we're just
                // deallocating right now).
                //
                // FIXME: This does mean that when we call allocate() to *grow* we might actually
                // deallocate our own memory. That's not ideal, but seems like an OK failure mode -
                // one workaround could be to pin the local page to avoid using it.

                // SAFETY: `count` is protected by the mutex we're in and pages are initialized to
                // a zero count when we move them out of free pages.
                let count = unsafe { page.as_ptr().cast::<u16>().read() };

                unsafe {
                    let mut next = page.as_ptr().add(PAGE_SIZE);
                    for _ in 0..count {
                        next = map_addr(next, |v| {
                            v.checked_sub(page_layout.size())
                                .map(|v| v & !(align - 1))
                                // should never overflow / underflow since we're iterating by `count`.
                                .unwrap()
                        });

                        // We prepend a u32 to layouts which contains the parent index.
                        let parent = next.cast::<u32>().read();

                        // Mark the parent as deallocated.
                        *self.parents[parent as usize].lock() = None;
                    }
                }

                // All entries on the page have been deallocated and are no longer in use, so this
                // page is now free.
                self.free_pages.push(page);

                // We don't need more than one page to be freed, so break out.
                continue 'allocate;
            }

            unreachable!("if no free pages must have at least some allocated pages")
        };

        // OK, we've allocated a block of memory (allocation). Now we need to initialize the parent
        // pointer.

        let parent_idx = self.parents.insert(parking_lot::Mutex::new(Some(
            u32::try_from(allocation as usize - self.region.as_ptr() as usize).unwrap(),
        )));
        unsafe {
            allocation
                .cast::<u32>()
                .write(u32::try_from(parent_idx).unwrap());
        }

        parent_idx
    }

    fn read_allocation<'a>(
        &self,
        parent: &'a Allocator,
        handle: usize,
    ) -> Option<AllocationGuard<'a>> {
        let guard = self.parents[handle].lock();
        let Some(offset) = *guard else {
            return None;
        };

        // FIXME: we leak this guard, and then release the `inner` mutex lock which is a bit
        // problematic since &mut could let you get_mut() on the Mutex... some safety condition is
        // probably missing somewhere.
        parking_lot::MutexGuard::leak(guard);

        Some(AllocationGuard {
            this: parent,
            mutex: handle,
            ptr: unsafe {
                NonNull::new(self.region.as_ptr().add(usize::try_from(offset).unwrap())).unwrap()
            },
        })
    }
}

#[cfg(miri)]
fn map_addr<T>(v: *mut T, mapper: impl FnOnce(usize) -> usize) -> *mut T {
    v.map_addr(mapper)
}

// Actually this is "new enough Rust", i.e., support for Strict Provenance.
// Remove when we bump MSRV to 1.84.
#[cfg(not(miri))]
fn map_addr<T>(v: *mut T, mapper: impl FnOnce(usize) -> usize) -> *mut T {
    mapper(v as usize) as *mut T
}

impl Drop for AllocatorInner {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.region.as_ptr(), self.layout);
        }
    }
}

#[cfg(test)]
mod test;
