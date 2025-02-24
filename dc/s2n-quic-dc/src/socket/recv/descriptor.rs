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
        Weak,
    },
};
use tracing::trace;

/// Callback which releases a descriptor back into the free list
pub(super) trait FreeList: 'static + Send + Sync {
    fn free(&self, descriptor: Descriptor);
}

/// A handle to various parts for the descriptor group instance
pub(super) struct Memory {
    capacity: u16,
    references: AtomicUsize,
    free_list: Weak<dyn FreeList>,
    #[allow(dead_code)]
    region: Box<dyn 'static + Send + Sync>,
}

impl Memory {
    pub(super) fn new<F: FreeList>(
        capacity: u16,
        free_list: Weak<F>,
        region: Box<dyn 'static + Send + Sync>,
    ) -> Box<Self> {
        Box::new(Self {
            capacity,
            references: AtomicUsize::new(0),
            free_list,
            region,
        })
    }
}

/// A pointer to a single descriptor in a group
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

    #[inline]
    pub(super) fn id(&self) -> u64 {
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
        self.inner().data
    }

    #[inline]
    fn upgrade(&self) {
        let inner = self.inner();
        trace!(upgrade = inner.id);
        inner.references.fetch_add(1, Ordering::Relaxed);
        unsafe {
            inner
                .memory
                .as_ref()
                .references
                .fetch_add(1, Ordering::Relaxed);
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

    #[inline]
    fn drop_filled(&self) {
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

        let mem = inner.free(self);

        trace!(free_desc = inner.id, state = %"filled");

        drop(mem);
    }

    #[inline]
    pub(super) fn drop_unfilled(&self) {
        let inner = self.inner();
        inner.references.store(0, Ordering::Release);
        let mem = inner.free(self);

        trace!(free_desc = inner.id, state = %"unfilled");

        drop(mem);
    }
}

unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

pub(super) struct DescriptorInner {
    id: u64,
    address: NonNull<Addr>,
    data: NonNull<u8>,

    references: AtomicUsize,

    memory: NonNull<Memory>,
}

impl DescriptorInner {
    pub(super) fn new(
        id: u64,
        address: NonNull<Addr>,
        data: NonNull<u8>,
        memory: NonNull<Memory>,
    ) -> Self {
        Self {
            id,
            address,
            data,
            references: AtomicUsize::new(0),
            memory,
        }
    }

    #[inline]
    fn capacity(&self) -> u16 {
        unsafe { self.memory.as_ref().capacity }
    }

    /// Frees the descriptor back into the pool
    #[inline]
    fn free(&self, desc: &Descriptor) -> Option<Box<Memory>> {
        let memory = unsafe { self.memory.as_ref() };
        let mem_refs = memory.references.fetch_sub(1, Ordering::Release);
        debug_assert_ne!(mem_refs, 0, "reference count underflow");

        // if the free_list is still active (the allocator hasn't dropped) then just push the id
        // TODO Weak::upgrade is a bit expensive since it clones the `Arc`, only to drop it again
        if let Some(free_list) = memory.free_list.upgrade() {
            free_list.free(Descriptor {
                ptr: desc.ptr,
                phantom: PhantomData,
            });
            return None;
        }

        // the free_list no longer active and we need to clean up the memory

        // based on the implementation in:
        // https://github.com/rust-lang/rust/blob/28b83ee59698ae069f5355b8e03f976406f410f5/library/alloc/src/sync.rs#L2551
        if mem_refs != 1 {
            trace!(memory_draining = mem_refs - 1, desc = self.id);
            return None;
        }

        core::sync::atomic::fence(Ordering::Acquire);

        trace!(memory_free = ?self.memory.as_ptr(), desc = self.id);

        // return the boxed memory rather than free it here - this works around
        // any stacked borrowing issues found by Miri
        Some(unsafe { Box::from_raw(self.memory.as_ptr()) })
    }
}

/// An unfilled packet
pub struct Unfilled {
    desc: Option<Descriptor>,
}

impl fmt::Debug for Unfilled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let desc = self.desc.as_ref().expect("invalid state");
        f.debug_struct("Unfilled").field("id", &desc.id()).finish()
    }
}

impl Unfilled {
    #[inline]
    pub(super) fn from_descriptor(desc: Descriptor) -> Self {
        desc.upgrade();
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
        let capacity = inner.capacity() as usize;
        let data = unsafe { core::slice::from_raw_parts_mut(inner.data.as_ptr(), capacity) };
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

        let segment_len = cmsg.segment_len();
        let ecn = cmsg.ecn();
        let desc = Filled {
            desc,
            offset: 0,
            len,
            ecn,
        };
        let segments = Segments {
            descriptor: Some(desc),
            segment_len,
        };
        Ok(segments)
    }
}

impl Drop for Unfilled {
    #[inline]
    fn drop(&mut self) {
        if let Some(desc) = self.desc.take() {
            // put the descriptor back in the pool if it wasn't filled
            desc.drop_unfilled();
        }
    }
}

pub struct Filled {
    desc: Descriptor,
    offset: u16,
    len: u16,
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
        Self {
            desc: self.desc.clone_filled(),
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
        self.desc.drop_filled()
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
