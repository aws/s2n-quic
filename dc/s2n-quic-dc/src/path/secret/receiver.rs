// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use self::allocator::Allocator;
use crate::credentials::{Credentials, KeyId};
use std::alloc::Layout;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

mod allocator;

#[derive(Debug)]
pub struct Shared {
    alloc: Allocator,
    entries: Mutex<slab::Slab<InnerState>>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    pub fn without_region() -> Arc<Shared> {
        Arc::new(Shared {
            alloc: Allocator::with_capacity(0),
            entries: Mutex::new(slab::Slab::new()),
        })
    }

    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            // ~20MB
            alloc: Allocator::with_capacity(20 * 1024 * 1024),
            entries: Mutex::new(slab::Slab::new()),
        })
    }

    pub fn new_receiver(self: Arc<Self>) -> State {
        let mut guard = self.entries.lock().unwrap();
        let key = guard.insert(InnerState::new());
        State {
            shared: self.clone(),
            entry: key,
        }
    }

    fn remove(&self, entry: usize) -> InnerState {
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(entry)
    }
}

#[derive(Debug)]
pub struct State {
    // FIXME: Avoid storing Shared pointer inside every path secret entry.
    // Instead thread the pointer through all the methods.
    shared: Arc<Shared>,
    // FIXME: shrink to u32 index?
    entry: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        let entry = self.shared.remove(self.entry);
        if let SharedIndex::Bitset(handle) | SharedIndex::Array(handle) = entry.shared.unpack() {
            // SAFETY: Entry is being dropped, so this is called at most once.
            unsafe { self.shared.alloc.deallocate(handle) };
        }
    }
}

// KeyIDs move through two filters:
//
// * `max_seen` + bitset absorbs traffic with minimal reordering. Conceptually they are a single
//   33-bit bitset ending at (inclusively) `max_seen`. 1-bits indicate seen entries. This is
//   currently expected to be enough to absorb the vast majority (>99.99%) of traffic seen in
//   practice. This space is always available to every Path Secret.
// * If we don't see a key ID (i.e., we shift out a zero bit from the bitset) we insert into a list
//   or bitset within the Shared state. This list tracks *only* unseen entries, so we expect it to
//   generally be short. Currently the list can track entries within a region 2**16 wide. Note that
//   this region is independent of `max_seen` and so only needs to potentially be changed if we
//   evict a zero bit (which happens pretty rarely), and even then only if we still haven't caught
//   a packet that's 2**16 old. See more details on `SortedListHeader` and `BitsetHeader`.
#[derive(Clone, Debug)]
pub struct InnerState {
    max_seen: u64,

    // Directly stored bitset, adjacent to max_seen.
    bitset: u32,

    // Any key ID > to this is either AlreadyExists or Ok.
    // Note that == is Unknown, since += 1 is *not* a safe operation.
    //
    // This is updated when we evict from the list/bitset (i.e., drop a still-Ok value).
    // FIXME: actually not updated today, because we need to thread this into deallocation for
    // proper updates.
    minimum_evicted: u64,

    // Index into the shared allocator's parents/entry array, if any.
    shared: SharedIndexMemory,

    // FIXME: Move into shared allocation.
    list: Vec<u64>,
}

// "u24" indices keep the in-memory size down.
#[derive(Copy, Clone)]
enum SharedIndexMemory {
    None,
    Array([u8; 3]),
    Bitset([u8; 3]),
}

impl std::fmt::Debug for SharedIndexMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.unpack().fmt(f)
    }
}

#[derive(Debug, Copy, Clone)]
enum SharedIndex {
    None,
    Array(usize),
    Bitset(usize),
}

impl SharedIndexMemory {
    fn unpack(self) -> SharedIndex {
        match self {
            SharedIndexMemory::None => SharedIndex::None,
            SharedIndexMemory::Array([a, b, c]) => {
                SharedIndex::Array(u32::from_le_bytes([a, b, c, 0]) as usize)
            }
            SharedIndexMemory::Bitset([a, b, c]) => {
                SharedIndex::Bitset(u32::from_le_bytes([a, b, c, 0]) as usize)
            }
        }
    }
}

impl SharedIndex {
    fn pack(self) -> SharedIndexMemory {
        match self {
            SharedIndex::None => SharedIndexMemory::None,
            SharedIndex::Array(i) => {
                assert!(i < (1 << 24));
                let [a, b, c, d] = (i as u32).to_le_bytes();
                assert!(d == 0);
                SharedIndexMemory::Array([a, b, c])
            }
            SharedIndex::Bitset(i) => {
                assert!(i < (1 << 24));
                let [a, b, c, d] = (i as u32).to_le_bytes();
                assert!(d == 0);
                SharedIndexMemory::Bitset([a, b, c])
            }
        }
    }
}

impl InnerState {
    fn new() -> Self {
        Self {
            max_seen: u64::MAX,
            minimum_evicted: u64::MAX,
            bitset: 0,
            shared: SharedIndexMemory::None,

            list: vec![],
        }
    }

    // Iterate over the unseen IDs that were > previous max seen, and
    // will not *become* tracked now (i.e., don't fall into the new bitset).
    //
    // The bitset tracks (max_seen-32)..=(max_seen-1)
    fn skipped_bitset(&self, previous_max: Option<u64>) -> std::ops::Range<u64> {
        let end = self.max_seen.saturating_sub(u32::BITS as u64);
        // Push start up so we don't push more than 65k elements, which is our list limit.
        // This avoids a too-long loop if we jump forward too much.
        let start = match previous_max {
            Some(previous_max) => (previous_max + 1).max(end.saturating_sub(u16::MAX as u64)),
            None => end.saturating_sub(u16::MAX as u64),
        };
        start..end
    }
}

impl State {
    pub fn without_shared() -> State {
        let shared = Shared::without_region();
        shared.new_receiver()
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> InnerState {
        self.shared.entries.lock().unwrap()[self.entry].clone()
    }

    pub fn with_shared(shared: Arc<Shared>) -> State {
        shared.new_receiver()
    }

    pub fn minimum_unseen_key_id(&self) -> KeyId {
        // wrapping_add ensures that our sentinel u64::MAX is zero, which is accurate (i.e., if we
        // have not seen any keys, then we have not seen the zeroth key either).
        KeyId::new(
            self.shared.entries.lock().unwrap()[self.entry]
                .max_seen
                .wrapping_add(1),
        )
        .unwrap()
    }

    pub fn pre_authentication(&self, _credentials: &Credentials) -> Result<(), Error> {
        // always pass for now
        Ok(())
    }

    pub fn post_authentication(&self, credentials: &Credentials) -> Result<(), Error> {
        let entry = &mut self.shared.entries.lock().unwrap()[self.entry];

        if entry.max_seen == u64::MAX {
            // no need to touch the bitset, we've not seen any of the previous entries.
            entry.max_seen = *credentials.key_id;

            for id in entry.skipped_bitset(None) {
                self.push_list(entry, id);
            }

            Ok(())
        } else if credentials.key_id > entry.max_seen {
            let previous_max = entry.max_seen;
            entry.max_seen = *credentials.key_id;
            let delta = entry.max_seen - previous_max;

            // This is the range that is going to get shifted out.
            //
            // Any bit not set means we haven't yet seen it, so we should add it to our list.
            //
            // If we shifted by 1, then the range we want is 31..=31 (1 bit, 1 << 31, top bit)
            // If we shifted by 2, then the range we want is 30..=31 (2 bits)
            // If we shifted by 30, then the range we want is 2..=31 (30 bits)
            // If we shifted by 60, then the range we want is 0..=31 (all 32 bits)
            for bit in (32u64.saturating_sub(delta)..=31).rev() {
                // +1 since bit 0 is previous_max - 1
                let Some(id) = previous_max.checked_sub(bit + 1) else {
                    continue;
                };
                if entry.bitset & (1 << bit) == 0 {
                    self.push_list(entry, id);
                }
            }

            for id in entry.skipped_bitset(Some(previous_max)) {
                self.push_list(entry, id);
            }

            if delta <= u32::BITS as u64 {
                // as u32 is safe since we checked we're less than 32.
                let delta = delta as u32;

                // Shift the no longer fitting bits out
                // 0s mean we have *not* seen the entry, so shifting those in for the middle part
                entry.bitset = entry.bitset.checked_shl(delta).unwrap_or(0);
                // Set the bit corresponding to previously max-seen.
                entry.bitset |= 1 << (delta - 1);
            } else {
                entry.bitset = 0;
            }

            // forward shift is always successful
            Ok(())
        } else if credentials.key_id == entry.max_seen {
            Err(Error::AlreadyExists)
        } else {
            let delta = entry.max_seen - *credentials.key_id;
            if delta <= u32::BITS as u64 {
                // -1 for the transition from max seen to the bitset
                if (entry.bitset & (1 << (delta - 1) as u32)) != 0 {
                    Err(Error::AlreadyExists)
                } else {
                    entry.bitset |= 1 << (delta - 1) as u32;
                    Ok(())
                }
            } else if let Ok(()) = self.try_remove_list(entry, *credentials.key_id) {
                Ok(())
            } else if *credentials.key_id > entry.minimum_evicted {
                Err(Error::AlreadyExists)
            } else {
                Err(Error::Unknown)
            }
        }
    }

    fn deallocate_shared(&self, entry: &mut InnerState) {
        if let SharedIndex::Bitset(handle) | SharedIndex::Array(handle) = entry.shared.unpack() {
            entry.shared = SharedIndexMemory::None;
            // SAFETY: we've cleared the shared field, so won't get called again.
            unsafe { self.shared.alloc.deallocate(handle) };
        }
    }

    fn push_list(&self, entry: &mut InnerState, id: u64) {
        for _ in 0..2 {
            match entry.shared.unpack() {
                SharedIndex::None => {
                    let guard = self.shared.alloc.allocate(SortedList::layout_for_cap(1));
                    entry.shared = SharedIndex::Array(guard.handle()).pack();
                    unsafe {
                        let mut list = SortedList::initialize(guard.as_ptr(), 1);
                        // Safe to unwrap because it can't need to grow -- we allocated with capacity
                        // for 1 element and that element will get used up here.
                        list.insert(id).unwrap();
                    }

                    // we're done, exit
                    return;
                }
                SharedIndex::Array(handle) => {
                    let Some(existing) = self.shared.alloc.read_allocation(handle) else {
                        self.deallocate_shared(entry);
                        // loop around to try again with a new allocation
                        continue;
                    };

                    let mut list = unsafe { SortedList::from_existing(existing.as_ptr()) };
                    let Err(err) = list.insert(id) else {
                        // successfully inserted, done.
                        return;
                    };

                    // drop the lock before we allocate, cannot hold entry lock across
                    // allocation or we may deadlock.
                    drop(existing);

                    let (_new_guard, mut list) = match err {
                        CapacityError::Array(cap) => {
                            let guard = self.shared.alloc.allocate(SortedList::layout_for_cap(cap));
                            entry.shared = SharedIndex::Array(guard.handle()).pack();
                            let list = unsafe {
                                SortedList::initialize(guard.as_ptr(), cap.try_into().unwrap())
                            };
                            (guard, list)
                        }
                        CapacityError::Bitset => {
                            todo!()
                        }
                    };

                    let previous = self.shared.alloc.read_allocation(handle);
                    if let Some(previous) = previous {
                        let mut prev_list = unsafe { SortedList::from_existing(previous.as_ptr()) };
                        prev_list.copy_to(&mut list);
                    }

                    // Safe to unwrap because it can't need to grow -- we allocated with
                    // capacity for at least one more element and that element will get used up
                    // here. We haven't released the lock on this list since allocation so it's
                    // impossible for some other thread to have used up the space.
                    //
                    // FIXME: that assumption is not true if we failed to copy, since we probably
                    // need to *shrink* then. Maybe we should allocate a temporary buffer to copy
                    // into?
                    list.insert(id).unwrap();

                    return;
                }
                SharedIndex::Bitset(_) => {
                    todo!()
                }
            }
        }

        // Should be unreachable - we should always exit from the loop in at most two "turns" via
        // `return`.
        unreachable!()
    }

    fn try_remove_list(&self, entry: &mut InnerState, id: u64) -> Result<(), ()> {
        if let Ok(idx) = entry.list.binary_search(&id) {
            // FIXME: augment with bitset for fast removal
            entry.list.remove(idx);
            Ok(())
        } else {
            Err(())
        }
    }
}

impl super::map::SizeOf for State {}

#[derive(Copy, Clone)]
struct SortedListHeader {
    len: u16,
    count: u16,
    cap: u16,
    minimum: u64,
}

struct SortedList {
    p: NonNull<u8>,
}

impl SortedList {
    unsafe fn initialize(ptr: NonNull<u8>, cap: u16) -> SortedList {
        ptr.as_ptr()
            .cast::<SortedListHeader>()
            .write(SortedListHeader {
                len: 0,
                count: 0,
                cap,
                minimum: 0,
            });
        SortedList { p: ptr.cast() }
    }

    fn layout_for_cap(cap: usize) -> Layout {
        Layout::new::<SortedListHeader>()
            .extend(Layout::array::<u16>(cap).unwrap())
            .unwrap()
            .0
            .extend(Layout::array::<u8>(cap.div_ceil(8)).unwrap())
            .unwrap()
            .0
    }
    fn bitset_offset(cap: usize) -> usize {
        Layout::new::<SortedListHeader>()
            .extend(Layout::array::<u16>(cap).unwrap())
            .unwrap()
            .0
            .extend(Layout::array::<u8>(cap.div_ceil(8)).unwrap())
            .unwrap()
            .1
    }

    fn slice_offset(cap: usize) -> usize {
        Layout::new::<SortedListHeader>()
            .extend(Layout::array::<u16>(cap).unwrap())
            .unwrap()
            .1
    }

    fn minimum(&self) -> u64 {
        // aligned to 8 bytes, so should be aligned.
        unsafe { self.p.cast::<SortedListHeader>().as_ref().minimum }
    }

    fn set_minimum(&self, min: u64) {
        unsafe {
            self.p.cast::<SortedListHeader>().as_mut().minimum = min;
        }
    }

    fn len(&self) -> usize {
        unsafe { usize::from(self.p.cast::<SortedListHeader>().as_ref().len) }
    }

    fn set_len(&self, len: usize) {
        unsafe {
            self.p.cast::<SortedListHeader>().as_mut().len = len.try_into().unwrap();
        }
    }

    fn capacity(&self) -> usize {
        unsafe { usize::from(self.p.cast::<SortedListHeader>().as_ref().cap) }
    }

    fn set_capacity(&self, cap: usize) {
        unsafe {
            self.p.cast::<SortedListHeader>().as_mut().cap = cap.try_into().unwrap();
        }
    }

    fn count(&self) -> usize {
        unsafe { usize::from(self.p.cast::<SortedListHeader>().as_ref().count) }
    }

    fn set_count(&self, count: usize) {
        unsafe {
            self.p.cast::<SortedListHeader>().as_mut().count = count.try_into().unwrap();
        }
    }

    #[inline(never)]
    fn insert(&mut self, value: u64) -> Result<(), CapacityError> {
        let value = match self.to_offset(value) {
            Some(v) => v,
            None => {
                self.compact_ensuring(value);
                self.to_offset(value).expect("compact ensuring guarantee")
            }
        };
        if self.len() == self.capacity() {
            // FIXME: might actually need to go to bitset or compact
            return Err(CapacityError::Array(self.len() + 1));
        }
        unsafe {
            // move past the header
            self.p
                .as_ptr()
                .add(Self::slice_offset(self.capacity()))
                .cast::<u16>()
                .add(self.len())
                .write(value);
            self.set_len(self.len() + 1);
            self.set_count(self.count() + 1);
        }

        Ok(())
    }

    #[inline(never)]
    fn remove(&mut self, value: u64) -> Result<(), Error> {
        let Some(value) = self.to_offset(value) else {
            // If the value is >= minimum, but we can't compute an offset, we know for sure that it
            // was not inserted into the array. As such it must have been received already.
            return if value >= self.minimum() {
                Err(Error::AlreadyExists)
            } else {
                Err(Error::Unknown)
            };
        };
        let slice = unsafe {
            std::slice::from_raw_parts::<u16>(
                self.p
                    .as_ptr()
                    .add(Self::slice_offset(self.capacity()))
                    .cast::<u16>(),
                self.len(),
            )
        };

        let Ok(idx) = slice.binary_search(&value) else {
            return Err(Error::Unknown);
        };
        let bitset = unsafe {
            std::slice::from_raw_parts_mut::<u8>(
                self.p.as_ptr().add(Self::bitset_offset(self.capacity())),
                self.len().div_ceil(8),
            )
        };
        let pos = idx / 8;
        let mask = 1 << (idx % 8);
        if bitset[pos] & mask != 0 {
            return Err(Error::AlreadyExists);
        }
        bitset[pos] |= mask;

        self.set_count(self.count() - 1);

        if self.count() * 2 < self.len() {
            self.shrink();
        }

        Ok(())
    }

    //fn grow(&mut self) {
    //    todo!()
    //    let new_cap = (self.capacity() + 1)
    //        .next_power_of_two()
    //        .clamp(0, u16::MAX as usize);
    //    self.reallocate_to(new_cap);
    //}

    fn copy_to(&mut self, new: &mut SortedList) {
        unsafe {
            let new_cap = new.capacity();
            let new = new.p;

            // copy header
            self.p
                .as_ptr()
                .copy_to_nonoverlapping(new.as_ptr(), std::mem::size_of::<SortedListHeader>());

            // copy bitset
            self.p
                .as_ptr()
                .add(Self::bitset_offset(self.capacity()))
                .copy_to_nonoverlapping(
                    new.as_ptr().add(Self::bitset_offset(new_cap)),
                    self.capacity().div_ceil(8),
                );

            // Zero out tail of the new bitset (that didn't get init'd by the copy above).
            std::slice::from_raw_parts_mut::<MaybeUninit<u8>>(
                new.as_ptr().add(Self::bitset_offset(new_cap)).cast(),
                new_cap.div_ceil(8),
            )[self.capacity().div_ceil(8)..]
                .fill(MaybeUninit::zeroed());

            // Copy the actual values
            self.p
                .as_ptr()
                .add(Self::slice_offset(self.capacity()))
                .cast::<u16>()
                .copy_to_nonoverlapping(
                    new.as_ptr().add(Self::slice_offset(new_cap)).cast(),
                    self.len(),
                );

            self.p = new;
            self.set_capacity(new_cap);
        }
    }

    // this also updates `minimum` to be best-possible given the data.
    fn shrink(&mut self) {
        todo!()
        //let slice = unsafe {
        //    std::slice::from_raw_parts::<u16>(
        //        self.p
        //            .as_ptr()
        //            .add(Self::slice_offset(self.capacity()))
        //            .cast::<u16>(),
        //        self.len(),
        //    )
        //};
        //let bitset = unsafe {
        //    std::slice::from_raw_parts::<u8>(
        //        self.p.as_ptr().add(Self::bitset_offset(self.capacity())),
        //        self.len().div_ceil(8),
        //    )
        //};

        //let mut new = Self::new();
        //let mut cap = 0;
        //while cap < self.count() {
        //    // should match grow()'s impl
        //    cap = (cap + 1).next_power_of_two().clamp(0, u16::MAX as usize);
        //}
        //new.reallocate_to(cap);
        //for (idx, value) in slice.iter().copied().enumerate() {
        //    let pos = idx / 8;
        //    let mask = 1 << (idx % 8);
        //    // not yet removed...
        //    if bitset[pos] & mask == 0 {
        //        new.insert(self.minimum() + value as u64);
        //    }
        //}
        //*self = new;
    }

    fn to_offset(&mut self, value: u64) -> Option<u16> {
        if self.minimum() == u64::MAX {
            self.set_minimum(value);
        }
        let value = value.checked_sub(self.minimum())?;
        u16::try_from(value).ok()
    }

    unsafe fn from_existing(p: NonNull<u8>) -> SortedList {
        SortedList { p }
    }

    /// Re-pack the sorted list, potentially dropping values, to ensure that `can_fit` fits into
    /// the list.
    fn compact_ensuring(&self, can_fit: u64) {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
enum CapacityError {
    #[error("need to grow or shrink to an array with capacity {0}")]
    Array(usize),
    #[error("need to grow or shrink to a bitset")]
    Bitset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// This indicates that we know about this element and it *definitely* already exists.
    #[error("packet definitely already seen before")]
    AlreadyExists,
    /// We don't know whether we've seen this element before. It may or may not have already been
    /// received.
    #[error("packet may have been seen before")]
    Unknown,
}

#[cfg(test)]
mod tests;
