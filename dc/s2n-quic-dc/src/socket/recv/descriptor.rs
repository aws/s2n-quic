// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::msg::{addr::Addr, cmsg};
use core::fmt;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::IoSliceMut,
    marker::PhantomData,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tracing::trace;

/// Callback which releases a descriptor back into the free list
pub(super) trait FreeList: 'static + Send + Sync {
    /// Frees a descriptor back into the free list
    ///
    /// Once the free list has been closed and all descriptors returned, the `free` function
    /// should return an object that can be dropped to release all of the memory associated
    /// with the descriptor pool. This works around any issues around the "Stacked Borrows"
    /// model by deferring freeing memory borrowed by `self`.
    fn free(&self, descriptor: Descriptor) -> Option<Box<dyn 'static + Send>>;
}

/// A pointer to a single descriptor in a group
///
/// Fundamentally, this is similar to something like `Arc<DescriptorInner>`. However,
/// unlike [`Arc`] which frees back to the global allocator, a Descriptor deallocates into
/// the backing [`FreeList`].
pub(super) struct Descriptor {
    ptr: NonNull<DescriptorInner>,
    phantom: PhantomData<DescriptorInner>,
}

impl Descriptor {
    #[inline]
    pub(super) fn new(ptr: NonNull<DescriptorInner>) -> Self {
        Self {
            ptr,
            phantom: PhantomData,
        }
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
    pub(super) fn id(&self) -> u32 {
        self.inner().id
    }

    #[inline]
    fn inner(&self) -> &DescriptorInner {
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    fn addr(&self) -> &Addr {
        unsafe { self.inner().address.as_ref() }
    }

    #[inline]
    fn data(&self) -> NonNull<u8> {
        self.inner().payload
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be exclusively owned
    /// * The provided `len` cannot exceed the allocated `capacity`
    #[inline]
    unsafe fn to_filled(self, len: u16, ecn: ExplicitCongestionNotification) -> Filled {
        let inner = self.inner();
        trace!(fill = inner.id, len, ?ecn);
        debug_assert!(len <= inner.capacity);

        // we can use relaxed since this only happens after it is filled, which was done by a single owner
        inner.references.store(1, Ordering::Relaxed);

        Filled {
            desc: self,
            offset: 0,
            len,
            ecn,
        }
    }

    #[inline]
    fn clone_filled(&self) -> Self {
        // https://github.com/rust-lang/rust/blob/28b83ee59698ae069f5355b8e03f976406f410f5/library/alloc/src/sync.rs#L2175
        // > Using a relaxed ordering is alright here, as knowledge of the
        // > original reference prevents other threads from erroneously deleting
        // > the object.
        let inner = self.inner();
        inner.references.fetch_add(1, Ordering::Relaxed);
        trace!(clone = inner.id);
        Self {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }

    /// # Safety
    ///
    /// * The descriptor must be in a filled state.
    /// * After calling this method, the descriptor handle should not be used
    #[inline]
    unsafe fn drop_filled(&self) {
        let inner = self.inner();
        let desc_ref = inner.references.fetch_sub(1, Ordering::Release);
        debug_assert_ne!(desc_ref, 0, "reference count underflow");

        // based on the implementation in:
        // https://github.com/rust-lang/rust/blob/28b83ee59698ae069f5355b8e03f976406f410f5/library/alloc/src/sync.rs#L2551
        if desc_ref != 1 {
            trace!(drop_desc_ref = inner.id);
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);

        let storage = inner.free(self);
        trace!(free_desc = inner.id, state = %"filled");
        drop(storage);
    }

    /// # Safety
    ///
    /// * The descriptor must be in an unfilled state.
    /// * After calling this method, the descriptor handle should not be used
    #[inline]
    unsafe fn drop_unfilled(&self) {
        let inner = self.inner();
        let storage = inner.free(self);
        trace!(free_desc = inner.id, state = %"unfilled");
        let _ = inner;
        drop(storage);
    }
}

unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

pub(super) struct DescriptorInner {
    /// An identifier for the descriptor
    ///
    /// This can be used by the pool implementation to map the descriptor to an internal
    /// detail, e.g. in AF_XDP it passes around UMEM offsets.
    id: u32,
    /// The maximum capacity for this descriptor
    capacity: u16,
    /// The pointer to the descriptor address
    address: NonNull<Addr>,
    /// The pointer to the descriptor payload
    payload: NonNull<u8>,
    /// The number of active references for this descriptor.
    ///
    /// This refcount allows for splitting descriptors into multiple segments
    /// and then correctly freeing the descriptor once the last segment is dropped.
    references: AtomicUsize,
    /// A reference back to the free list
    free_list: Arc<dyn FreeList>,
}

impl DescriptorInner {
    /// # Safety
    ///
    /// `address` must be a valid pointer (i.e., safe to create `&Addr` from it with lifetime bounded by the `Arc<dyn FreeList>`)
    ///
    /// `payload` must point to a valid region of memory that is at least `capacity` bytes
    /// long. Additionally it must be initialized to valid memory.
    ///
    /// `memory` must be initialized.
    pub(super) unsafe fn new(
        id: u32,
        capacity: u16,
        address: NonNull<Addr>,
        payload: NonNull<u8>,
        free_list: Arc<dyn FreeList>,
    ) -> Self {
        Self {
            id,
            capacity,
            address,
            payload,
            references: AtomicUsize::new(0),
            free_list,
        }
    }

    /// Frees the descriptor back into the pool
    ///
    /// # Safety
    ///
    /// * The descriptor must not be referenced (`references == 0`)
    #[inline]
    unsafe fn free(&self, desc: &Descriptor) -> Option<Box<dyn 'static + Send>> {
        debug_assert_eq!(desc.inner().references.load(Ordering::Relaxed), 0);
        self.free_list.free(Descriptor {
            ptr: desc.ptr,
            phantom: PhantomData,
        })
    }
}

/// An unfilled packet
pub struct Unfilled {
    /// The inner raw descriptor.
    ///
    /// This needs to be an [`Option`] to allow for both consuming the descriptor
    /// into a [`Filled`] after receiving a packet or dropping the [`Unfilled`] and
    /// releasing it back into the packet pool.
    desc: Option<Descriptor>,
}

impl fmt::Debug for Unfilled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let desc = self.desc.as_ref().expect("invalid state");
        f.debug_struct("Unfilled").field("id", &desc.id()).finish()
    }
}

impl Unfilled {
    /// Creates an [`Unfilled`] descriptor from a raw [`Descriptor`].
    #[inline]
    pub(super) fn from_descriptor(desc: Descriptor) -> Self {
        Self { desc: Some(desc) }
    }

    /// Fills the packet with the given callback, if the callback is successful
    #[inline]
    pub fn recv_with<F, E>(mut self, f: F) -> Result<Segments, (Self, E)>
    where
        F: FnOnce(&mut Addr, &mut cmsg::Receiver, IoSliceMut) -> Result<usize, E>,
    {
        let desc = self.desc.take().expect("invalid state");
        let inner = desc.inner();
        let addr = unsafe { &mut *inner.address.as_ptr() };
        let capacity = inner.capacity as usize;
        let data = unsafe {
            // SAFETY: a pool implementation is required to initialize all payload bytes
            core::slice::from_raw_parts_mut(inner.payload.as_ptr(), capacity)
        };
        let iov = IoSliceMut::new(data);
        let mut cmsg = cmsg::Receiver::default();

        let len = match f(addr, &mut cmsg, iov) {
            Ok(len) => {
                debug_assert!(len <= capacity);
                len.min(capacity) as u16
            }
            Err(err) => {
                let unfilled = Self { desc: Some(desc) };
                return Err((unfilled, err));
            }
        };

        let desc = unsafe {
            // SAFETY: the descriptor is exclusively owned here and the returned len does not exceed
            //         the allowed capacity
            desc.to_filled(len, cmsg.ecn())
        };
        let segments = Segments {
            descriptor: Some(desc),
            segment_len: cmsg.segment_len(),
        };
        Ok(segments)
    }
}

impl Drop for Unfilled {
    #[inline]
    fn drop(&mut self) {
        if let Some(desc) = self.desc.take() {
            // put the descriptor back in the pool if it wasn't filled
            unsafe {
                // SAFETY: the descriptor is in the `unfilled` state and no longer used
                desc.drop_unfilled();
            }
        }
    }
}

/// A filled packet
pub struct Filled {
    /// The raw descriptor
    desc: Descriptor,
    /// The offset into the payload
    ///
    /// This allows for splitting up a filled packet into multiple segments, while still ensuring
    /// exclusive access to a region.
    offset: u16,
    /// The filled length of the payload
    len: u16,
    /// The ECN marking of the packet
    ecn: ExplicitCongestionNotification,
}

impl fmt::Debug for Filled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let alt = f.alternate();

        let mut s = f.debug_struct("Filled");
        s.field("id", &self.desc.id())
            .field("remote_address", &self.remote_address().get())
            .field("ecn", &self.ecn);

        if alt {
            s.field("payload", &self.payload());
        } else {
            s.field("payload_len", &self.len);
        }

        s.finish()
    }
}

impl Filled {
    /// Returns the ECN markings for the packet
    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    /// Returns the length of the payload
    #[inline]
    pub fn len(&self) -> u16 {
        self.len
    }

    /// Returns `true` if the payload is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the remote address of the packet
    #[inline]
    pub fn remote_address(&self) -> &Addr {
        // NOTE: addr_mut can't be used since the `inner` is reference counted to allow for GRO
        self.desc.addr()
    }

    /// Returns the packet payload
    #[inline]
    pub fn payload(&self) -> &[u8] {
        unsafe {
            // SAFETY: the descriptor has been filled through the [`Unfilled`] API
            let ptr = self.desc.data().as_ptr().add(self.offset as _);
            let len = self.len as usize;
            core::slice::from_raw_parts(ptr, len)
        }
    }

    /// Returns a mutable packet payload
    // NOTE: this is safe since we guarantee no `Filled` references overlap
    #[inline]
    pub fn payload_mut(&mut self) -> &mut [u8] {
        unsafe {
            // SAFETY: the descriptor has been filled through the [`Unfilled`] API
            // SAFETY: the `offset` + `len` are exclusively owned by this reference
            let ptr = self.desc.data().as_ptr().add(self.offset as _);
            let len = self.len as usize;
            core::slice::from_raw_parts_mut(ptr, len)
        }
    }

    /// Splits the packet into two at the given index.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned
    /// [`Filled`] contains elements `[0, at)`.
    ///
    /// This is an `O(1)` operation that just increases the reference count and
    /// sets a few indices.
    ///
    /// # Panics
    ///
    /// Panics if `at > len`.
    #[must_use = "consider Filled::advance if you don't need the other half"]
    #[inline]
    pub fn split_to(&mut self, at: u16) -> Self {
        assert!(at <= self.len);
        let offset = self.offset;
        let ecn = self.ecn;
        self.offset += at;
        self.len -= at;

        // Update the reference counts for the descriptor.
        //
        // Even if one of the lengths is set to 0, we still need to increment the
        // reference count, since the descriptor can still access the `remote_address`.
        let desc = self.desc.clone_filled();

        Self {
            desc,
            offset,
            len: at,
            ecn,
        }
    }

    /// Shortens the packet, keeping the first `len` bytes and dropping the
    /// rest.
    ///
    /// If `len` is greater than the packet's current length, this has no
    /// effect.
    #[inline]
    pub fn truncate(&mut self, len: u16) {
        self.len = len.min(self.len);
    }

    /// Advances the start of the packet by `len`
    ///
    /// # Panics
    ///
    /// This function panics if `len > self.len()`
    #[inline]
    pub fn advance(&mut self, len: u16) {
        assert!(len <= self.len);
        self.offset += len;
        self.len -= len;
    }
}

impl Drop for Filled {
    #[inline]
    fn drop(&mut self) {
        // decrement the reference count, which may put the descriptor back into the pool once
        // it reaches 0
        unsafe {
            // SAFETY: the descriptor is in the `filled` state and the handle is no longer used
            self.desc.drop_filled()
        }
    }
}

/// An iterator over all of the filled segments in a packet
///
/// This is used for when the socket interface allows for receiving multiple packets
/// in a single syscall, e.g. GRO.
pub struct Segments {
    descriptor: Option<Filled>,
    segment_len: u16,
}

impl Iterator for Segments {
    type Item = Filled;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // if the segment length wasn't specified, then just return the entire thing
        if self.segment_len == 0 {
            return self.descriptor.take();
        }

        let descriptor = self.descriptor.as_mut()?;

        // if the current descriptor exceeds the segment length then we need to split it off in bump
        // the reference counts
        if descriptor.len() > self.segment_len {
            return Some(descriptor.split_to(self.segment_len as _));
        }

        // the segment len was bigger than the overall descriptor so return the whole thing to avoid
        // reference count churn
        self.descriptor.take()
    }
}
