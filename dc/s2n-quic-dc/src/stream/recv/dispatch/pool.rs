// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    descriptor::{Control, Descriptor, DescriptorInner, FreeList, Stream},
    sender::{Sender, SenderEntry, SenderPages, Senders},
};
use crate::sync::mpsc::{self, Capacity};
use s2n_quic_core::varint::VarInt;
use std::{
    alloc::Layout,
    marker::PhantomData,
    ptr::NonNull,
    sync::{Arc, Mutex, RwLock},
};

pub struct Pool<T: 'static + Send, const PAGE_SIZE: usize> {
    free: Arc<Free<T>>,
    senders: Arc<RwLock<SenderPages<T>>>,
    stream_capacity: Capacity,
    control_capacity: Capacity,
    epoch: VarInt,
}

impl<T: 'static + Send, const PAGE_SIZE: usize> Clone for Pool<T, PAGE_SIZE> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            free: self.free.clone(),
            senders: self.senders.clone(),
            stream_capacity: self.stream_capacity,
            control_capacity: self.control_capacity,
            epoch: self.epoch,
        }
    }
}

impl<T: 'static + Send, const PAGE_SIZE: usize> Pool<T, PAGE_SIZE> {
    #[inline]
    pub fn new(stream_capacity: Capacity, control_capacity: Capacity) -> Self {
        let free = Free::new(PAGE_SIZE);
        let mut pool = Pool {
            free,
            senders: Default::default(),
            stream_capacity,
            control_capacity,
            epoch: VarInt::ZERO,
        };
        pool.grow();
        pool
    }

    #[inline]
    pub fn senders(&self) -> Senders<T, PAGE_SIZE> {
        Senders {
            senders: self.senders.clone(),
            local: Default::default(),
        }
    }

    #[inline]
    pub fn alloc(&self) -> Option<(Control<T>, Stream<T>)> {
        self.free.alloc()
    }

    #[inline]
    pub fn alloc_or_grow(&mut self) -> (Control<T>, Stream<T>) {
        loop {
            if let Some(descriptor) = self.free.alloc() {
                return descriptor;
            }
            self.grow();
        }
    }

    #[inline(never)] // this should happen rarely
    fn grow(&mut self) {
        let (region, layout) = Region::alloc(PAGE_SIZE);

        let ptr = region.ptr;

        let mut pending_desc = vec![];
        let mut pending_senders = vec![];

        for idx in 0..PAGE_SIZE {
            let offset = layout.size() * idx;

            let (stream_s, stream_r) = mpsc::new(self.stream_capacity);
            let (control_s, control_r) = mpsc::new(self.control_capacity);

            unsafe {
                let descriptor = ptr.as_ptr().add(offset).cast::<DescriptorInner<T>>();
                // initialize the descriptor - note that it is self-referential to `addr`, `data`, and `free`
                // SAFETY: address, payload, and memory are all initialized
                descriptor.write(DescriptorInner::new(
                    self.epoch + idx,
                    stream_r,
                    control_r,
                    self.free.clone(),
                ));

                let descriptor = NonNull::new_unchecked(descriptor);

                // push the descriptor into the free list
                pending_desc.push(Descriptor::new(descriptor));

                // push the senders into the sender page
                let sender = SenderEntry {
                    inner: Sender {
                        stream: stream_s,
                        control: control_s,
                    },
                    descriptor: Descriptor::new(descriptor),
                };
                pending_senders.push(sender);
            }
        }

        let pending_senders: Arc<[_]> = pending_senders.into();

        let mut senders = self.senders.write().unwrap();

        // check if another pool instance already updated the senders list
        if senders.epoch != self.epoch {
            // update our local copy
            self.epoch = senders.epoch;

            // free what we just allocated, since we raced with the other pool instance
            for desc in pending_desc {
                unsafe {
                    desc.drop_in_place();
                }
            }

            // return back to the alloc method, which may have a free descriptor now
            return;
        }

        // update the epoch with the latest value
        let target_epoch = self.epoch + PAGE_SIZE;
        senders.epoch = target_epoch;
        self.epoch = target_epoch;

        // update the sender list with the newly allocated channels
        senders.pages.push(pending_senders);
        // we don't need to synchronize with the senders any more so drop the local
        drop(senders);

        // push all of the descriptors into the free list
        self.free.record_region(region, pending_desc);
    }
}

impl<T: 'static + Send, const PAGE_SIZE: usize> Drop for Pool<T, PAGE_SIZE> {
    #[inline]
    fn drop(&mut self) {
        let _ = self.free.close();
    }
}

struct Region<T: 'static> {
    ptr: NonNull<u8>,
    layout: Layout,
    phantom: PhantomData<T>,
}

unsafe impl<T: Send> Send for Region<T> {}
unsafe impl<T: Sync> Sync for Region<T> {}

impl<T: 'static> Region<T> {
    #[inline]
    fn alloc(page_size: usize) -> (Self, Layout) {
        debug_assert!(page_size > 0, "need at least 1 entry in page");

        // first create the descriptor layout
        let descriptor = Layout::new::<DescriptorInner<T>>().pad_to_align();

        let descriptors = {
            // TODO use `descriptor.repeat(page_size)` once stable
            // https://doc.rust-lang.org/stable/core/alloc/struct.Layout.html#method.repeat
            Layout::from_size_align(
                descriptor.size().checked_mul(page_size).unwrap(),
                descriptor.align(),
            )
            .unwrap()
        };

        let ptr = unsafe {
            // SAFETY: the layout is non-zero size
            debug_assert_ne!(descriptors.size(), 0);
            // ensure that the allocation is zeroed out so we don't have to worry about MaybeUninit
            std::alloc::alloc_zeroed(descriptors)
        };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| std::alloc::handle_alloc_error(descriptors));

        let region = Self {
            ptr,
            layout: descriptors,
            phantom: PhantomData,
        };

        (region, descriptor)
    }
}

impl<T> Drop for Region<T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

/// A free list of unfilled descriptors
///
/// Note that this uses a [`Vec`] instead of [`std::collections::VecDeque`], which acts more
/// like a stack than a queue. This is to prefer more-recently used descriptors which should
/// hopefully reduce the number of cache misses.
struct Free<T: 'static>(Mutex<FreeInner<T>>);

impl<T: 'static> Free<T> {
    #[inline]
    fn new(initial_cap: usize) -> Arc<Self> {
        let descriptors = Vec::with_capacity(initial_cap);
        let regions = Vec::with_capacity(1);
        let inner = FreeInner {
            descriptors,
            regions,
            total: 0,
            open: true,
        };
        Arc::new(Self(Mutex::new(inner)))
    }

    #[inline]
    fn alloc(&self) -> Option<(Control<T>, Stream<T>)> {
        self.0.lock().unwrap().descriptors.pop().map(|v| unsafe {
            // SAFETY: the descriptor is only owned by the free list
            v.into_owned()
        })
    }

    #[inline]
    fn record_region(&self, region: Region<T>, mut descriptors: Vec<Descriptor<T>>) {
        let mut inner = self.0.lock().unwrap();
        inner.regions.push(region);
        inner.total += descriptors.len();
        inner.descriptors.append(&mut descriptors);
        // Even though the `descriptors` is now empty (`len=0`), it still owns
        // capacity and will need to be freed. Drop the lock before interacting
        // with the global allocator.
        drop(inner);
        drop(descriptors);
    }

    #[inline]
    fn close(&self) -> Option<FreeInner<T>> {
        let mut inner = self.0.lock().unwrap();
        inner.open = false;
        inner.try_free()
    }
}

impl<T: 'static + Send> FreeList<T> for Free<T> {
    #[inline]
    fn free(&self, descriptor: Descriptor<T>) -> Option<Box<dyn 'static + Send>> {
        let mut inner = self.0.lock().unwrap();
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
}

impl<T: 'static> FreeInner<T> {
    #[inline(never)] // this is rarely called
    fn try_free(&mut self) -> Option<Self> {
        if self.descriptors.len() < self.total {
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

        tracing::trace!("releasing free list");

        for descriptor in self.descriptors.drain(..) {
            unsafe {
                // SAFETY: the free list is closed and there are no outstanding descriptors
                descriptor.drop_in_place();
            }
        }
    }
}
