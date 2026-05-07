// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    descriptor::Descriptor,
    handle::{Control, Stream},
    pool::Region,
    probes,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// Callback which releases a descriptor back into the free list
pub(super) trait FreeList<S, C, Key>: 'static + Send + Sync {
    /// Frees a descriptor back into the free list
    ///
    /// Once the free list has been closed and all descriptors returned, the `free` function
    /// should return an object that can be dropped to release all of the memory associated
    /// with the descriptor pool. This works around any issues around the "Stacked Borrows"
    /// model by deferring freeing memory borrowed by `self`.
    fn free(&self, descriptor: Descriptor<S, C, Key>) -> Option<Box<dyn 'static + Send>>;
}

/// A free list of unfilled descriptors
///
/// Note that this uses a [`Vec`] instead of [`std::collections::VecDeque`], which acts more
/// like a stack than a queue. This is to prefer more-recently used descriptors which should
/// hopefully reduce the number of cache misses.
pub(super) struct FreeVec<S: 'static, C: 'static, Key: 'static> {
    inner: Mutex<FreeInner<S, C, Key>>,
}

impl<S: 'static, C: 'static, Key: 'static> FreeVec<S, C, Key> {
    #[inline]
    pub fn new(initial_cap: usize) -> (Arc<Self>, Arc<Memory<S, C, Key>>) {
        let descriptors = VecDeque::with_capacity(initial_cap);
        let regions = Vec::with_capacity(1);
        let inner = FreeInner {
            descriptors,
            regions,
            total: 0,
            open: true,
            #[cfg(debug_assertions)]
            active: Default::default(),
        };
        let inner = Mutex::new(inner);
        let free = Arc::new(Self { inner });
        let memory = Arc::new(Memory(free.clone()));
        (free, memory)
    }

    #[inline]
    pub fn alloc(&self, key: Key) -> Result<(Control<S, C, Key>, Stream<S, C, Key>), Key> {
        let mut inner = self.inner.lock().unwrap();
        let Some(descriptor) = inner.descriptors.pop_front() else {
            return Err(key);
        };

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
            descriptor.init_key(key);
            let (control, stream) = descriptor.into_receiver_pair();
            Ok((Control::new(control), Stream::new(stream)))
        }
    }

    #[inline]
    pub fn record_region(
        &self,
        region: Region<S, C, Key>,
        descriptors: Vec<Descriptor<S, C, Key>>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.regions.push(region);
        let prev = inner.total;
        let next = prev + descriptors.len();
        inner.total = next;
        let mut descriptors: VecDeque<_> = descriptors.into();
        inner.descriptors.append(&mut descriptors);
        // Even though the `descriptors` is now empty (`len=0`), it still owns
        // capacity and will need to be freed. Drop the lock before interacting
        // with the global allocator.
        drop(inner);
        drop(descriptors);
        probes::on_grow(prev, next);
    }

    #[inline]
    fn try_free(&self) -> Option<FreeInner<S, C, Key>> {
        let mut inner = self.inner.lock().unwrap();
        inner.open = false;
        inner.try_free()
    }
}

/// A memory reference to the free list
///
/// Once dropped, the pool and all associated descriptors will be
/// freed after the last handle is dropped.
pub(super) struct Memory<S: 'static, C: 'static, Key: 'static>(Arc<FreeVec<S, C, Key>>);

impl<S: 'static, C: 'static, Key: 'static> Drop for Memory<S, C, Key> {
    #[inline]
    fn drop(&mut self) {
        drop(self.0.try_free());
    }
}

impl<S, C, Key> FreeList<S, C, Key> for FreeVec<S, C, Key>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    Key: 'static + Send + Sync,
{
    #[inline]
    fn free(&self, descriptor: Descriptor<S, C, Key>) -> Option<Box<dyn 'static + Send>> {
        let mut inner = self.inner.lock().unwrap();

        #[cfg(debug_assertions)]
        assert!(
            inner.active.remove(&descriptor.as_usize()),
            "{} not in {:?}",
            descriptor.as_usize(),
            inner.active
        );

        inner.descriptors.push_back(descriptor);
        if inner.open {
            return None;
        }
        inner
            .try_free()
            .map(|to_free| Box::new(to_free) as Box<dyn 'static + Send>)
    }
}

struct FreeInner<S: 'static, C: 'static, Key: 'static> {
    descriptors: VecDeque<Descriptor<S, C, Key>>,
    regions: Vec<Region<S, C, Key>>,
    total: usize,
    open: bool,
    #[cfg(debug_assertions)]
    active: std::collections::BTreeSet<usize>,
}

impl<S: 'static, C: 'static, Key: 'static> FreeInner<S, C, Key> {
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
                descriptors: VecDeque::new(),
                regions: Vec::new(),
                total: 0,
                open: false,
                #[cfg(debug_assertions)]
                active: Default::default(),
            },
        ))
    }
}

impl<S: 'static, C: 'static, Key: 'static> Drop for FreeInner<S, C, Key> {
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
