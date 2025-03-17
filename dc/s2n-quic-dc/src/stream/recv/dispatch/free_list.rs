// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    descriptor::Descriptor,
    handle::{Control, Stream},
    pool::Region,
    probes,
};
use std::sync::{Arc, Mutex};

/// Callback which releases a descriptor back into the free list
pub(super) trait FreeList<T>: 'static + Send + Sync {
    /// Frees a descriptor back into the free list
    ///
    /// Once the free list has been closed and all descriptors returned, the `free` function
    /// should return an object that can be dropped to release all of the memory associated
    /// with the descriptor pool. This works around any issues around the "Stacked Borrows"
    /// model by deferring freeing memory borrowed by `self`.
    fn free(&self, descriptor: Descriptor<T>) -> Option<Box<dyn 'static + Send>>;
}

/// A free list of unfilled descriptors
///
/// Note that this uses a [`Vec`] instead of [`std::collections::VecDeque`], which acts more
/// like a stack than a queue. This is to prefer more-recently used descriptors which should
/// hopefully reduce the number of cache misses.
pub(super) struct FreeVec<T: 'static>(Mutex<FreeInner<T>>);

impl<T: 'static> FreeVec<T> {
    #[inline]
    pub fn new(initial_cap: usize) -> (Arc<Self>, Arc<Memory<T>>) {
        let descriptors = Vec::with_capacity(initial_cap);
        let regions = Vec::with_capacity(1);
        let inner = FreeInner {
            descriptors,
            regions,
            total: 0,
            open: true,
            #[cfg(debug_assertions)]
            active: Default::default(),
        };
        let free = Arc::new(Self(Mutex::new(inner)));
        let memory = Arc::new(Memory(free.clone()));
        (free, memory)
    }

    #[inline]
    pub fn alloc(&self) -> Option<(Control<T>, Stream<T>)> {
        let mut inner = self.0.lock().unwrap();
        let descriptor = inner.descriptors.pop()?;

        #[cfg(debug_assertions)]
        assert!(
            inner.active.insert(descriptor.as_usize()),
            "{} already in {:?}",
            descriptor.as_usize(),
            inner.active
        );

        drop(inner);

        unsafe {
            // SAFETY: the descriptor is only owned by the free list
            let (control, stream) = descriptor.into_receiver_pair();
            Some((Control::new(control), Stream::new(stream)))
        }
    }

    #[inline]
    pub fn record_region(&self, region: Region<T>, mut descriptors: Vec<Descriptor<T>>) {
        let mut inner = self.0.lock().unwrap();
        inner.regions.push(region);
        let prev = inner.total;
        let next = prev + descriptors.len();
        inner.total = next;
        inner.descriptors.append(&mut descriptors);
        // Even though the `descriptors` is now empty (`len=0`), it still owns
        // capacity and will need to be freed. Drop the lock before interacting
        // with the global allocator.
        drop(inner);
        drop(descriptors);
        probes::on_grow(prev, next);
    }

    #[inline]
    fn try_free(&self) -> Option<FreeInner<T>> {
        let mut inner = self.0.lock().unwrap();
        inner.open = false;
        inner.try_free()
    }
}

/// A memory reference to the free list
///
/// Once dropped, the pool and all associated descriptors will be
/// freed after the last handle is dropped.
pub(super) struct Memory<T: 'static>(Arc<FreeVec<T>>);

impl<T: 'static> Drop for Memory<T> {
    #[inline]
    fn drop(&mut self) {
        drop(self.0.try_free());
    }
}

impl<T: 'static + Send> FreeList<T> for FreeVec<T> {
    #[inline]
    fn free(&self, descriptor: Descriptor<T>) -> Option<Box<dyn 'static + Send>> {
        let mut inner = self.0.lock().unwrap();

        #[cfg(debug_assertions)]
        assert!(
            inner.active.remove(&descriptor.as_usize()),
            "{} not in {:?}",
            descriptor.as_usize(),
            inner.active
        );

        inner.descriptors.push(descriptor);
        if inner.open {
            return None;
        }
        inner
            .try_free()
            .map(|to_free| Box::new(to_free) as Box<dyn 'static + Send>)
    }
}

struct FreeInner<T: 'static> {
    descriptors: Vec<Descriptor<T>>,
    regions: Vec<Region<T>>,
    total: usize,
    open: bool,
    #[cfg(debug_assertions)]
    active: std::collections::BTreeSet<usize>,
}

impl<T: 'static> FreeInner<T> {
    #[inline(never)] // this is rarely called
    fn try_free(&mut self) -> Option<Self> {
        #[cfg(debug_assertions)]
        assert_eq!(self.total - self.descriptors.len(), self.active.len());

        if self.descriptors.len() < self.total {
            probes::on_draining(self.total, self.total - self.descriptors.len());
            return None;
        }

        // move all of the allocations out of itself, since this is self-referential
        Some(core::mem::replace(
            self,
            FreeInner {
                descriptors: Vec::new(),
                regions: Vec::new(),
                total: 0,
                open: false,
                #[cfg(debug_assertions)]
                active: Default::default(),
            },
        ))
    }
}

impl<T: 'static> Drop for FreeInner<T> {
    #[inline]
    fn drop(&mut self) {
        if self.descriptors.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        assert!(self.active.is_empty());

        probes::on_drained(self.total);

        for descriptor in self.descriptors.drain(..) {
            unsafe {
                // SAFETY: the free list is closed and there are no outstanding descriptors
                descriptor.drop_in_place();
            }
        }
    }
}
