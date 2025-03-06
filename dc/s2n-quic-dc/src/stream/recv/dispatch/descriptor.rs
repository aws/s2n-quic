// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::mpsc;
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_core::{sync::CachePadded, varint::VarInt};
use std::{
    marker::PhantomData,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tracing::trace;

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

    #[inline]
    pub(super) fn is_active(&self) -> bool {
        self.inner().references.load(Ordering::Relaxed) > 0
    }

    /// # Safety
    ///
    /// This should only be called once the caller can guarantee the descriptor is no longer
    /// used.
    #[inline]
    pub(super) unsafe fn drop_in_place(&self) {
        core::ptr::drop_in_place(self.ptr.as_ptr());
    }

    #[inline]
    fn inner(&self) -> &DescriptorInner<T> {
        unsafe { self.ptr.as_ref() }
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be exclusively owned
    #[inline]
    pub(super) unsafe fn into_owned(self) -> (Control<T>, Stream<T>) {
        let inner = self.inner();

        // we can use relaxed since this only happens after it is filled, which was done by a single owner
        inner.references.store(2, Ordering::Relaxed);

        let stream = Stream(Self {
            ptr: self.ptr,
            phantom: PhantomData,
        });
        let control = Control(self);

        (control, stream)
    }

    /// # Safety
    ///
    /// * The descriptor must be in an owned state.
    /// * After calling this method, the descriptor handle should not be used
    #[inline]
    unsafe fn drop_owned(&self) {
        let inner = self.inner();
        let desc_ref = inner.references.fetch_sub(1, Ordering::Release);
        debug_assert_ne!(desc_ref, 0, "reference count underflow");

        // based on the implementation in:
        // https://github.com/rust-lang/rust/blob/28b83ee59698ae069f5355b8e03f976406f410f5/library/alloc/src/sync.rs#L2551
        if desc_ref != 1 {
            trace!(drop_desc_ref = ?inner.id);
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);

        // drain any remaining items
        for recv in [&inner.control, &inner.stream] {
            while let Ok(Some(item)) = recv.try_recv_front() {
                drop(item);
            }
        }

        let storage = inner.free_list.free(Descriptor {
            ptr: self.ptr,
            phantom: PhantomData,
        });
        trace!(free_desc = ?inner.id, state = %"owned");
        drop(storage);
    }
}

unsafe impl<T: Send> Send for Descriptor<T> {}
unsafe impl<T: Sync> Sync for Descriptor<T> {}

pub(super) struct DescriptorInner<T> {
    id: VarInt,
    references: CachePadded<AtomicUsize>,
    stream: CachePadded<mpsc::Receiver<T>>,
    control: CachePadded<mpsc::Receiver<T>>,
    /// A reference back to the free list
    free_list: Arc<dyn FreeList<T>>,
}

impl<T> DescriptorInner<T> {
    pub(super) fn new(
        id: VarInt,
        stream: mpsc::Receiver<T>,
        control: mpsc::Receiver<T>,
        free_list: Arc<dyn FreeList<T>>,
    ) -> Self {
        Self {
            id,
            stream: CachePadded::new(stream),
            control: CachePadded::new(control),
            references: CachePadded::new(AtomicUsize::new(0)),
            free_list,
        }
    }
}

macro_rules! impl_recv {
    ($name:ident, $field:ident) => {
        pub struct $name<T: 'static>(Descriptor<T>);

        impl<T: 'static> $name<T> {
            #[inline]
            pub fn queue_id(&self) -> VarInt {
                self.0.inner().id
            }

            #[inline]
            pub fn try_recv(&self) -> Result<Option<T>, mpsc::Closed> {
                self.0.inner().$field.try_recv_front()
            }

            #[inline]
            pub fn poll_recv(&self, cx: &mut Context) -> Poll<Result<T, mpsc::Closed>> {
                self.0.inner().$field.poll_recv_front(cx)
            }

            #[inline]
            pub fn poll_swap(
                &self,
                cx: &mut Context,
                out: &mut std::collections::VecDeque<T>,
            ) -> Poll<Result<(), mpsc::Closed>> {
                self.0.inner().$field.poll_swap(cx, out)
            }
        }

        impl<T: 'static> fmt::Debug for $name<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("queue_id", &self.queue_id())
                    .finish()
            }
        }

        impl<T: 'static> Drop for $name<T> {
            #[inline]
            fn drop(&mut self) {
                unsafe {
                    self.0.drop_owned();
                }
            }
        }
    };
}

impl_recv!(Control, control);
impl_recv!(Stream, stream);

impl<T: 'static> Stream<T> {
    #[inline]
    pub fn sender(&self) -> StreamSender<T> {
        StreamSender(self.0.inner().stream.sender())
    }
}

pub struct StreamSender<T>(mpsc::Sender<T>);

impl<T> Clone for StreamSender<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static> StreamSender<T> {
    #[inline]
    pub fn send(&self, item: T) -> Result<Option<T>, mpsc::Closed> {
        self.0.send_back(item)
    }
}
