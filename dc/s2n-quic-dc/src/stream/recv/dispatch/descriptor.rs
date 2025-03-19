// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    free_list::FreeList,
    probes,
    queue::{Half, Queue},
};
use crate::sync::ring_deque;
use s2n_quic_core::{ensure, varint::VarInt};
use std::{
    marker::PhantomData,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// A pointer to a single descriptor in a group
///
/// Fundamentally, this is similar to something like `Arc<DescriptorInner>`. However,
/// unlike [`Arc`] which frees back to the global allocator, a Descriptor deallocates into
/// the backing [`FreeList`].
pub(super) struct Descriptor<T> {
    ptr: NonNull<DescriptorInner<T>>,
    phantom: PhantomData<DescriptorInner<T>>,
}

impl<T: 'static> Descriptor<T> {
    #[inline]
    pub(super) fn new(ptr: NonNull<DescriptorInner<T>>) -> Self {
        Self {
            ptr,
            phantom: PhantomData,
        }
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated. Additionally,
    /// the [`Self::drop_sender`] method should be used when the cloned descriptor is
    /// no longer needed.
    #[inline]
    pub unsafe fn clone_for_sender(&self) -> Descriptor<T> {
        self.inner().senders.fetch_add(1, Ordering::Relaxed);
        Descriptor::new(self.ptr)
    }

    /// # Safety
    ///
    /// This should only be called once the caller can guarantee the descriptor is no longer
    /// used.
    #[inline]
    pub unsafe fn drop_in_place(&self) {
        core::ptr::drop_in_place(self.ptr.as_ptr());
    }

    #[cfg(debug_assertions)]
    pub(super) fn as_usize(&self) -> usize {
        // TODO use `.addr()` once MSRV is 1.84
        self.ptr.as_ptr() as usize
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn queue_id(&self) -> VarInt {
        self.inner().id
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn stream_queue(&self) -> &Queue<T> {
        &self.inner().stream
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn control_queue(&self) -> &Queue<T> {
        &self.inner().control
    }

    #[inline]
    fn inner(&self) -> &DescriptorInner<T> {
        unsafe { self.ptr.as_ref() }
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be marked as free of receivers
    #[inline]
    pub unsafe fn into_receiver_pair(self) -> (Self, Self) {
        let inner = self.inner();

        // open the queues back up for receiving
        inner.stream.open_receivers(&inner.control).unwrap();

        probes::on_receiver_open(inner.id);

        let other = Self {
            ptr: self.ptr,
            phantom: PhantomData,
        };

        (self, other)
    }

    /// # Safety
    ///
    /// This method can be used to drop the Descriptor, but shouldn't be called after the last sender Descriptor
    /// is released. That implies only calling it once on a given Descriptor handle obtained from [`Self::clone_for_sender`].
    #[inline]
    pub unsafe fn drop_sender(&self) {
        let inner = self.inner();
        let desc_ref = inner.senders.fetch_sub(1, Ordering::Release);
        debug_assert_ne!(desc_ref, 0, "reference count underflow");

        // based on the implementation in:
        // https://github.com/rust-lang/rust/blob/28b83ee59698ae069f5355b8e03f976406f410f5/library/alloc/src/sync.rs#L2551
        if desc_ref != 1 {
            probes::on_sender_drop(inner.id);
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);

        // close both of the queues so the receivers are notified
        inner.control.close();
        inner.stream.close();

        probes::on_sender_close(inner.id);
    }

    /// # Safety
    ///
    /// This method can be used to drop the Descriptor, but shouldn't be called after the last receiver Descriptor
    /// is released. That implies only calling it once on a given Descriptor handle obtained from [`Self::into_receiver_pair`].
    #[inline]
    pub unsafe fn drop_receiver(&self, half: Half) {
        let inner = self.inner();
        probes::on_receiver_drop(inner.id, half);

        ensure!(inner
            .stream
            .close_receiver(&inner.control, half)
            .is_continue());

        probes::on_receiver_free(inner.id, half);
        let storage = inner.free_list.free(Descriptor {
            ptr: self.ptr,
            phantom: PhantomData,
        });
        drop(storage);
    }
}

unsafe impl<T: Send> Send for Descriptor<T> {}
unsafe impl<T: Sync> Sync for Descriptor<T> {}

pub(super) struct DescriptorInner<T> {
    id: VarInt,
    stream: Queue<T>,
    control: Queue<T>,
    /// A reference back to the free list
    free_list: Arc<dyn FreeList<T>>,
    senders: AtomicUsize,
}

impl<T> DescriptorInner<T> {
    pub(super) fn new(
        id: VarInt,
        stream: ring_deque::Capacity,
        control: ring_deque::Capacity,
        free_list: Arc<dyn FreeList<T>>,
    ) -> Self {
        let stream = Queue::new(stream, Half::Stream);
        let control = Queue::new(control, Half::Control);
        Self {
            id,
            stream,
            control,
            senders: AtomicUsize::new(0),
            free_list,
        }
    }
}
