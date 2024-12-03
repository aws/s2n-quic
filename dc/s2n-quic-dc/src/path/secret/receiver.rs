// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::credentials::{Credentials, Id, KeyId};
use s2n_quic_core::packet::number::{
    PacketNumber, PacketNumberSpace, SlidingWindow, SlidingWindowError,
};
use std::{
    cell::UnsafeCell,
    ptr::NonNull,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const SHARED_ENTRIES: usize = 1 << 20;
// Maximum page size on current machines (macOS aarch64 has 16kb pages)
//
// mmap is documented as failing if we don't request a page boundary. Currently our sizes work out
// such that rounding is useless, but this is good future proofing.
const MAX_PAGE: usize = 16_384;
const SHARED_ALLOCATION: usize = {
    let element = std::mem::size_of::<SharedSlot>();
    let size = element * SHARED_ENTRIES;
    // TODO use `next_multiple_of` once MSRV is >=1.73
    (size + MAX_PAGE - 1) / MAX_PAGE * MAX_PAGE
};

#[derive(Debug)]
pub struct Shared {
    secret: u64,
    backing: NonNull<SharedSlot>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe {
            if libc::munmap(self.backing.as_ptr().cast(), SHARED_ALLOCATION) != 0 {
                // Avoid panicking in a destructor, just let the memory leak while logging. We
                // expect this to be essentially a global singleton in most production cases so
                // likely we're exiting the process anyway.
                eprintln!(
                    "Failed to unmap memory: {:?}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

const fn assert_copy<T: Copy>() {}

struct SharedSlot {
    id: UnsafeCell<Id>,
    key_id: AtomicU64,
}

impl SharedSlot {
    fn try_lock(&self) -> Option<SharedSlotGuard<'_>> {
        let current = self.key_id.load(Ordering::Relaxed);
        if current & LOCK != 0 {
            // If we are already locked, then give up.
            // A concurrent thread updated this slot, any write we do would squash that thread's
            // write. Doing so if that thread remove()d may make sense in the future but not right
            // now.
            return None;
        }
        let Ok(_) = self.key_id.compare_exchange(
            current,
            current | LOCK,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) else {
            return None;
        };

        Some(SharedSlotGuard {
            slot: self,
            key_id: current,
        })
    }
}

struct SharedSlotGuard<'a> {
    slot: &'a SharedSlot,
    key_id: u64,
}

impl SharedSlotGuard<'_> {
    fn write_id(&mut self, id: Id) {
        // Store the new ID.
        // SAFETY: We hold the lock since we are in the guard.
        unsafe {
            // Note: no destructor is run for the previously stored element, but Id is Copy.
            // If we did want to run a destructor we'd have to ensure that we replaced a PRESENT
            // entry.
            assert_copy::<Id>();
            std::ptr::write(self.slot.id.get(), id);
        }
    }

    fn id(&self) -> Id {
        // SAFETY: We hold the lock, so copying out the Id is safe.
        unsafe { *self.slot.id.get() }
    }
}

impl Drop for SharedSlotGuard<'_> {
    fn drop(&mut self) {
        self.slot.key_id.store(self.key_id, Ordering::Release);
    }
}

const LOCK: u64 = 1 << 62;
const PRESENT: u64 = 1 << 63;

impl Shared {
    pub fn new() -> Arc<Shared> {
        let mut secret = [0; 8];
        aws_lc_rs::rand::fill(&mut secret).expect("random is available");
        let shared = Shared {
            secret: u64::from_ne_bytes(secret),
            backing: unsafe {
                // Note: We rely on the zero-initialization provided by the kernel. That ensures
                // that an entry in the map is not LOCK'd to begin with and is not PRESENT as well.
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    SHARED_ALLOCATION,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
                    0,
                    0,
                );
                // -1
                if ptr as usize == usize::MAX {
                    panic!(
                        "Failed to allocate backing allocation for shared: {:?}",
                        std::io::Error::last_os_error()
                    );
                }
                NonNull::new(ptr).unwrap().cast()
            },
        };

        // We need to modify the slot to which an all-zero path secert ID and key ID map. Otherwise
        // we'd return Err(AlreadyExists) for that entry which isn't correct - it has not been
        // inserted or removed, so it should be Err(Unknown).
        //
        // This is the only slot that needs modification. All other slots are never used for lookup
        // of this set of credentials and so containing this set of credentials is fine.
        let slot = shared.slot(&Credentials {
            id: Id::from([0; 16]),
            key_id: KeyId::new(0).unwrap(),
        });
        // The max key ID is never used by senders (checked on the sending side), while avoiding
        // taking a full bit out of the range of key IDs. We also statically return Unknown for it
        // on removal to avoid a non-local invariant.
        slot.key_id.store(KeyId::MAX.as_u64(), Ordering::Relaxed);

        Arc::new(shared)
    }

    pub fn new_receiver(self: Arc<Shared>) -> State {
        State::with_shared(self)
    }

    fn insert(&self, identity: &Credentials) {
        let slot = self.slot(identity);
        let Some(mut guard) = slot.try_lock() else {
            return;
        };
        guard.write_id(identity.id);
        guard.key_id = *identity.key_id | PRESENT;
    }

    fn remove(&self, identity: &Credentials) -> Result<(), Error> {
        // See `new` for details.
        if identity.key_id == KeyId::MAX.as_u64() {
            return Err(Error::Unknown);
        }

        let slot = self.slot(identity);
        let previous = slot.key_id.load(Ordering::Relaxed);
        if previous & LOCK != 0 {
            // If we are already locked, then give up.
            // A concurrent thread updated this slot, any write we do would squash that thread's
            // write. No concurrent thread could have inserted what we're looking for since
            // both insert and remove for a single path secret ID run under a Mutex.
            return Err(Error::Unknown);
        }
        if previous & (!PRESENT) != *identity.key_id {
            // If the currently stored entry does not match our desired KeyId,
            // then we don't know whether this key has been replayed or not.
            return Err(Error::Unknown);
        }

        let Some(mut guard) = slot.try_lock() else {
            // Don't try to win the race by spinning, let the other thread proceed.
            return Err(Error::Unknown);
        };

        // Check if the path secret ID matches.
        if guard.id() != identity.id {
            return Err(Error::Unknown);
        }

        // Ok, at this point we know that the key ID and the path secret ID both match.

        let ret = if guard.key_id & PRESENT != 0 {
            Ok(())
        } else {
            Err(Error::AlreadyExists)
        };

        // Release the lock, removing the PRESENT bit (which may already be missing).
        guard.key_id = *identity.key_id;

        ret
    }

    fn index(&self, identity: &Credentials) -> usize {
        let hash = u64::from_ne_bytes(identity.id[..8].try_into().unwrap())
            ^ *identity.key_id
            ^ self.secret;
        let index = hash & (SHARED_ENTRIES as u64 - 1);
        index as usize
    }

    fn slot(&self, identity: &Credentials) -> &SharedSlot {
        let index = self.index(identity);
        // SAFETY: in-bounds -- the & above truncates such that we're always in the appropriate
        // range that we allocated with mmap above.
        //
        // Casting to a reference is safe -- the Slot type has an UnsafeCell around all of the data
        // (either inside the atomic or directly).
        unsafe { self.backing.as_ptr().add(index).as_ref().unwrap_unchecked() }
    }
}

#[derive(Debug)]
pub struct State {
    // Minimum that we're potentially willing to accept.
    // This is lazily updated and so may be out of date.
    min_key_id: AtomicU64,

    // This is the maximum ID we've seen so far. This is sent to peers for when we cannot determine
    // if the packet sent is replayed as it falls outside our replay window. Peers use this
    // information to resynchronize on the latest state.
    max_seen_key_id: AtomicU64,

    seen: Mutex<SlidingWindow>,

    shared: Option<Arc<Shared>>,
}

impl super::map::SizeOf for Mutex<SlidingWindow> {
    fn size(&self) -> usize {
        // If we don't need drop, it's very likely that this type is fully contained in size_of
        // Self. This simplifies implementing this trait for e.g. std types.
        //
        // Mutex on macOS (at least) has a more expensive, pthread-based impl that allocates. But
        // on Linux there's no extra allocation.
        if cfg!(target_os = "linux") {
            assert!(
                !std::mem::needs_drop::<Self>(),
                "{:?} requires custom SizeOf impl",
                std::any::type_name::<Self>()
            );
        }
        std::mem::size_of::<Self>()
    }
}

impl super::map::SizeOf for State {
    fn size(&self) -> usize {
        let State {
            min_key_id,
            max_seen_key_id,
            seen,
            shared,
        } = self;
        // shared is shared across all State's (effectively) so we don't currently account for that
        // allocation.
        min_key_id.size() + max_seen_key_id.size() + seen.size() + std::mem::size_of_val(shared)
    }
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

impl State {
    pub fn without_shared() -> State {
        State {
            min_key_id: Default::default(),
            max_seen_key_id: Default::default(),
            seen: Default::default(),
            shared: None,
        }
    }

    pub fn with_shared(shared: Arc<Shared>) -> State {
        State {
            min_key_id: Default::default(),
            max_seen_key_id: Default::default(),
            seen: Default::default(),
            shared: Some(shared),
        }
    }

    pub fn pre_authentication(&self, identity: &Credentials) -> Result<(), Error> {
        if self.min_key_id.load(Ordering::Relaxed) > *identity.key_id {
            return Err(Error::Unknown);
        }

        Ok(())
    }

    pub fn minimum_unseen_key_id(&self) -> KeyId {
        KeyId::try_from(self.max_seen_key_id.load(Ordering::Relaxed) + 1).unwrap()
    }

    /// Called after decryption has been performed
    pub fn post_authentication(&self, identity: &Credentials) -> Result<(), Error> {
        let key_id = identity.key_id;
        self.max_seen_key_id.fetch_max(*key_id, Ordering::Relaxed);
        let pn = PacketNumberSpace::Initial.new_packet_number(key_id);

        // Note: intentionally retaining this lock across potential insertion into the shared map.
        // This avoids the case where we have evicted an entry but cannot see it in the shared map
        // yet from a concurrent thread. This should not be required for correctness but helps
        // reasoning about the state of the world.
        let mut seen = self.seen.lock().unwrap();
        match seen.insert_with_evicted(pn) {
            Ok(evicted) => {
                if let Some(shared) = &self.shared {
                    // FIXME: Consider bounding the number of evicted entries to insert or
                    // otherwise optimizing? This can run for at most 128 entries today...
                    for evicted in evicted {
                        shared.insert(&Credentials {
                            id: identity.id,
                            key_id: PacketNumber::as_varint(evicted),
                        });
                    }
                }
                Ok(())
            }
            Err(SlidingWindowError::TooOld) => {
                if let Some(shared) = &self.shared {
                    shared.remove(identity)
                } else {
                    Err(Error::Unknown)
                }
            }
            Err(SlidingWindowError::Duplicate) => Err(Error::AlreadyExists),
        }
    }
}

#[cfg(test)]
mod tests;
