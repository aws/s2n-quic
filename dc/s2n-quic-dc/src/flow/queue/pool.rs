// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    descriptor::{Descriptor, DescriptorInner},
    free_list::{self, FreeVec},
    handle::{Control, Sender, Stream},
    sender::{self, Senders},
};
use s2n_quic_core::varint::VarInt;
use std::{alloc::Layout, marker::PhantomData, ptr::NonNull, sync::Arc};

pub struct Pool<S: 'static + Send, C: 'static + Send, Key: 'static + Send, const PAGE_SIZE: usize> {
    pub(super) senders: Arc<sender::State<S, C, Key>>,
    free: Arc<FreeVec<S, C, Key>>,
    /// Holds the backing memory allocated as long as there's at least one reference
    memory_handle: Arc<free_list::Memory<S, C, Key>>,
    epoch: VarInt,
    base: VarInt,
}

impl<S: 'static + Send, C: 'static + Send, Key: 'static + Send, const PAGE_SIZE: usize> Clone
    for Pool<S, C, Key, PAGE_SIZE>
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            free: self.free.clone(),
            memory_handle: self.memory_handle.clone(),
            senders: self.senders.clone(),
            epoch: self.epoch,
            base: self.base,
        }
    }
}

impl<S, C, Key, const PAGE_SIZE: usize> Pool<S, C, Key, PAGE_SIZE>
where
    S: 'static + Send + Sync,
    C: 'static + Send + Sync,
    Key: 'static + Send + Sync,
{
    #[inline]
    pub fn new() -> Self {
        let epoch = VarInt::ZERO;
        let senders = sender::State::new(epoch);
        let (free, memory_handle) = FreeVec::new(PAGE_SIZE);
        let mut pool = Pool {
            free,
            memory_handle,
            senders,
            epoch,
            base: epoch,
        };
        pool.grow();
        pool
    }

    #[inline]
    pub fn senders(&self) -> Senders<S, C, Key, PAGE_SIZE> {
        Senders {
            state: self.senders.clone(),
            // make sure the memory lives as long as this sender is alive
            memory_handle: self.memory_handle.clone(),
            local: Default::default(),
            base: self.base,
        }
    }

    #[inline]
    pub fn alloc(
        &self,
        key: Key,
        remote_queue_id: Option<VarInt>,
    ) -> Result<(Control<S, C, Key>, Stream<S, C, Key>), Key> {
        self.free.alloc(key, remote_queue_id)
    }

    #[inline]
    pub fn alloc_or_grow(
        &mut self,
        mut key: Key,
        remote_queue_id: Option<VarInt>,
    ) -> (Control<S, C, Key>, Stream<S, C, Key>) {
        loop {
            match self.alloc(key, remote_queue_id) {
                Ok(descriptor) => return descriptor,
                Err(k) => {
                    key = k;
                    self.grow();
                }
            }
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

            unsafe {
                let descriptor = ptr
                    .as_ptr()
                    .add(offset)
                    .cast::<DescriptorInner<S, C, Key>>();

                // Give the descriptor a non-`Strong` reference to the free list, since this will be the
                // last reference to get dropped.
                let free_list = self.free.clone();

                // initialize the descriptor with the channels
                descriptor.write(DescriptorInner::new(self.epoch + idx, free_list));

                let descriptor = NonNull::new_unchecked(descriptor);
                let descriptor = Descriptor::new(descriptor);
                let sender = Sender::new(descriptor.clone_for_sender());

                // push the descriptor into the free list
                pending_desc.push(descriptor);

                // push the senders into the sender page
                pending_senders.push(sender);
            }
        }

        let pending_senders: Arc<[_]> = pending_senders.into();

        let mut senders = self.senders.pages.write().unwrap();

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

            drop(senders);
            tracing::debug!("grow failed");

            // return back to the alloc method, which may have a free descriptor now
            return;
        }

        // update the epoch with the latest value
        let target_epoch = self.epoch + PAGE_SIZE;
        senders.epoch = target_epoch;
        self.epoch = target_epoch;

        // update the sender list with the newly allocated channels
        senders.pages.push(pending_senders);
        let epoch = senders.epoch;
        // we don't need to synchronize with the senders any more so drop the local
        drop(senders);

        tracing::debug!(%epoch, "grow");

        // push all of the descriptors into the free list
        self.free.record_region(region, pending_desc);
    }
}

pub(super) struct Region<S: 'static, C: 'static, Key: 'static> {
    ptr: NonNull<u8>,
    layout: Layout,
    phantom: PhantomData<(S, C, Key)>,
}

unsafe impl<S: Send, C: Send, Key: Send> Send for Region<S, C, Key> {}
unsafe impl<S: Sync, C: Sync, Key: Sync> Sync for Region<S, C, Key> {}

impl<S: 'static, C: 'static, Key: 'static> Region<S, C, Key> {
    #[inline]
    fn alloc(page_size: usize) -> (Self, Layout) {
        debug_assert!(page_size > 0, "need at least 1 entry in page");

        // first create the descriptor layout
        let descriptor = Layout::new::<DescriptorInner<S, C, Key>>().pad_to_align();

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

impl<S, C, Key> Drop for Region<S, C, Key> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}
