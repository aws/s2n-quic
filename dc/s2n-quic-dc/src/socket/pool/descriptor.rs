// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator,
    msg::{self, addr::Addr, cmsg},
};
use core::fmt;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    alloc::Layout,
    io::{IoSlice, IoSliceMut},
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use tracing::trace;

/// A pointer to a single descriptor
///
/// Each descriptor owns a contiguous allocation containing:
/// `[Header | Addr | payload bytes]`
///
/// Reference counting allows splitting a filled descriptor into multiple
/// segments (e.g., for GRO). When the last reference is dropped, the entire
/// allocation is freed through [`allocator::packet::dealloc`].
pub(super) struct Descriptor {
    ptr: NonNull<Header>,
}

impl Descriptor {
    /// Allocates a new descriptor with the given payload `capacity`.
    ///
    /// Returns `None` if the packet subheap is exhausted.
    #[inline]
    fn alloc(capacity: u16) -> Option<Self> {
        let (layout, addr_offset, _payload_offset) = Header::layout(capacity);

        let ptr = allocator::packet::alloc(layout)?;

        unsafe {
            let base = ptr.as_ptr();

            // Payload is uninitialized - callers must fill before reading

            // Initialize the Header at the start of the allocation
            let inner_ptr = base.cast::<Header>();
            inner_ptr.write(Header {
                capacity,
                references: AtomicUsize::new(1),
            });

            // Initialize the Addr
            let addr_ptr = base.add(addr_offset).cast::<Addr>();
            addr_ptr.write(Addr::default());

            Some(Self {
                ptr: NonNull::new_unchecked(inner_ptr),
            })
        }
    }

    #[inline]
    fn inner(&self) -> &Header {
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    fn addr(&self) -> &Addr {
        unsafe {
            let base = self.ptr.as_ptr().cast::<u8>();
            &*base.add(self.inner().addr_offset()).cast::<Addr>()
        }
    }

    #[inline]
    fn data(&self) -> NonNull<u8> {
        unsafe {
            let base = self.ptr.as_ptr().cast::<u8>();
            NonNull::new_unchecked(base.add(self.inner().payload_offset()))
        }
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be exclusively owned
    /// * The provided `len` cannot exceed the allocated `capacity`
    #[inline]
    unsafe fn into_filled(self, len: u16, ecn: ExplicitCongestionNotification) -> Filled {
        let inner = self.inner();
        trace!(fill = ?self.ptr, len, ?ecn);
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
        trace!(clone = ?self.ptr);
        Self { ptr: self.ptr }
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
            trace!(drop_desc_ref = ?self.ptr);
            return;
        }

        core::sync::atomic::fence(Ordering::Acquire);

        trace!(free_desc = ?self.ptr, state = %"filled");
        self.dealloc();
    }

    /// # Safety
    ///
    /// * The descriptor must be in an unfilled state.
    /// * After calling this method, the descriptor handle should not be used
    #[inline]
    unsafe fn drop_unfilled(&self) {
        trace!(free_desc = ?self.ptr, state = %"unfilled");
        self.dealloc();
    }

    /// Deallocates the entire contiguous allocation for this descriptor.
    ///
    /// # Safety
    ///
    /// Must only be called when there are no remaining references.
    #[inline]
    unsafe fn dealloc(&self) {
        let inner = self.inner();
        let (layout, _addr_offset, _payload_offset) = Header::layout(inner.capacity);
        let base = self.ptr.cast::<u8>();

        // Static assertion: Header and Addr must not have non-trivial Drop impls,
        // since we dealloc without dropping them.
        const {
            assert!(!core::mem::needs_drop::<Header>());
            assert!(!core::mem::needs_drop::<Addr>());
        }

        allocator::packet::dealloc(base, layout);
    }
}

unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

struct Header {
    /// The maximum capacity for this descriptor
    capacity: u16,
    /// The number of active references for this descriptor.
    ///
    /// This refcount allows for splitting descriptors into multiple segments
    /// and then correctly freeing the descriptor once the last segment is dropped.
    references: AtomicUsize,
}

impl Header {
    /// Computes the layout for the contiguous allocation:
    /// `[Header | Addr | payload bytes]`
    ///
    /// Returns `(layout, addr_offset, payload_offset)`.
    #[inline]
    const fn layout(capacity: u16) -> (Layout, usize, usize) {
        let inner = Layout::new::<Header>();
        let Ok((with_addr, addr_offset)) = inner.extend(Layout::new::<Addr>()) else {
            panic!("not enough space for addr");
        };
        let Ok(payload_layout) = Layout::array::<u8>(capacity as usize) else {
            panic!("not enough space for payload");
        };
        let Ok((with_payload, payload_offset)) = with_addr.extend(payload_layout) else {
            panic!("not enough space for payload");
        };
        (with_payload.pad_to_align(), addr_offset, payload_offset)
    }

    #[inline]
    const fn addr_offset(&self) -> usize {
        Self::layout(self.capacity).1
    }

    #[inline]
    const fn payload_offset(&self) -> usize {
        Self::layout(self.capacity).2
    }
}

/// An unfilled packet
pub struct Unfilled {
    /// The inner raw descriptor.
    ///
    /// This needs to be an [`Option`] to allow for both consuming the descriptor
    /// into a [`Filled`] after receiving a packet or dropping the [`Unfilled`] and
    /// releasing it back into the packet allocator.
    desc: Option<Descriptor>,
}

impl fmt::Debug for Unfilled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Unfilled").finish()
    }
}

impl Unfilled {
    /// Allocates a new unfilled packet with the given payload capacity.
    ///
    /// Returns `None` if the packet subheap is exhausted.
    #[inline]
    pub fn new(capacity: u16) -> Option<Self> {
        let desc = Descriptor::alloc(capacity)?;
        Some(Self { desc: Some(desc) })
    }

    /// Fills the packet with the given callback, if the callback is successful
    #[inline]
    pub fn fill_with<F, E>(mut self, f: F) -> Result<Segments, (Self, E)>
    where
        F: FnOnce(&mut Addr, &mut cmsg::Receiver, IoSliceMut) -> Result<usize, E>,
    {
        let desc = self.desc.take().expect("invalid state");
        let capacity = desc.inner().capacity as usize;
        let addr = unsafe {
            let base = desc.ptr.as_ptr().cast::<u8>();
            &mut *base.add(desc.inner().addr_offset()).cast::<Addr>()
        };
        let data = unsafe {
            // SAFETY: the payload region was allocated with at least `capacity` bytes
            let base = desc.ptr.as_ptr().cast::<u8>();
            core::slice::from_raw_parts_mut(base.add(desc.inner().payload_offset()), capacity)
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
            desc.into_filled(len, cmsg.ecn())
        };
        let segments = Segments {
            descriptor: desc,
            segment_len: cmsg.segment_len(),
        };
        Ok(segments)
    }
}

impl Drop for Unfilled {
    #[inline]
    fn drop(&mut self) {
        if let Some(desc) = self.desc.take() {
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
        f.debug_struct("Filled")
            .field(
                "remote_address",
                &format_args!("{}", self.remote_address().get()),
            )
            .field("ecn", &self.ecn)
            .field("payload_len", &self.len)
            .finish()
    }
}

impl Filled {
    /// Returns the ECN markings for the packet
    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    pub fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.ecn = ecn;
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

impl core::ops::Deref for Filled {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.payload()
    }
}

impl core::ops::DerefMut for Filled {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.payload_mut()
    }
}

impl s2n_quic_core::buffer::reader::Storage for Filled {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len() as usize
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.is_empty()
    }

    #[inline]
    fn read_chunk(
        &mut self,
        watermark: usize,
    ) -> Result<s2n_quic_core::buffer::reader::storage::Chunk<'_>, Self::Error> {
        use s2n_quic_core::buffer::reader::storage::Chunk;
        let len = (self.len() as usize).min(watermark);
        if len == 0 {
            return Ok(Chunk::empty());
        }
        // SAFETY: The descriptor's backing allocation is ref-counted and will not be
        // freed while `self` is alive.  `advance` only updates bookkeeping (offset/len)
        // and does not move or invalidate the underlying memory.  The returned slice
        // is valid for the exclusive-borrow lifetime of `self` because the backing
        // allocation lives at least as long as `self`.
        let slice: &[u8] = unsafe { core::slice::from_raw_parts(self.payload().as_ptr(), len) };
        self.advance(len as u16);
        Ok(slice.into())
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<s2n_quic_core::buffer::reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: s2n_quic_core::buffer::writer::Storage + ?Sized,
    {
        self.read_chunk(dest.remaining_capacity())
    }
}

impl Filled {
    /// Creates a deep copy of this filled descriptor by allocating a new buffer
    /// and copying the payload bytes. This is safe because the copy gets its own
    /// exclusive memory region.
    pub fn deep_copy(&self) -> Option<Self> {
        let Some(unfilled) = Unfilled::new(self.len) else {
            return None;
        };
        let payload = self.payload();
        let result = unfilled.fill_with(|addr, _cmsg, mut iov| {
            iov[..payload.len()].copy_from_slice(payload);
            addr.set(self.remote_address().get());
            <Result<_, core::convert::Infallible>>::Ok(payload.len())
        });
        match result {
            Ok(segments) => {
                let mut filled = segments.take_filled();
                filled.set_ecn(self.ecn);
                Some(filled)
            }
            Err(_) => None,
        }
    }
}

impl Drop for Filled {
    #[inline]
    fn drop(&mut self) {
        // decrement the reference count, which may deallocate the descriptor once
        // it reaches 0
        unsafe {
            // SAFETY: the descriptor is in the `filled` state and the handle is no longer used
            self.desc.drop_filled()
        }
    }
}

pub struct Segments {
    descriptor: Filled,
    segment_len: u16,
}

impl fmt::Debug for Segments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segments")
            .field("payload_len", &self.descriptor.len())
            .field("segment_len", &self.segment_len)
            .finish()
    }
}

impl Segments {
    pub fn new(descriptor: Filled, segment_len: u16) -> Self {
        let segment_len = if segment_len == 0 {
            descriptor.len()
        } else {
            segment_len.min(descriptor.len())
        };
        Self {
            descriptor,
            segment_len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.descriptor.len() == 0
    }

    pub fn total_payload_len(&self) -> u16 {
        self.descriptor.len()
    }

    pub fn take_filled(self) -> Filled {
        self.descriptor
    }

    pub fn send_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Addr, ExplicitCongestionNotification, &[IoSlice]) -> R,
    {
        let addr = self.descriptor.remote_address();
        let ecn = self.descriptor.ecn();
        let payload = self.descriptor.payload();

        let segment_len = if self.segment_len == 0 {
            payload.len()
        } else {
            payload.len().min(self.segment_len as _)
        };

        debug_assert!(payload.len().div_ceil(segment_len) <= msg::segment::MAX_COUNT);

        let mut segments = [IoSlice::new(&[]); msg::segment::MAX_COUNT];

        let mut count = 0;
        for (segment, ioslice) in payload.chunks(segment_len).zip(segments.iter_mut()) {
            *ioslice = IoSlice::new(segment);
            count += 1;
        }

        let segments = &segments[..count];
        f(addr, ecn, segments)
    }
}

impl crate::socket::channel::ByteCost for Segments {
    fn byte_cost(&self) -> u64 {
        self.descriptor.len() as u64
    }
}

impl crate::socket::channel::Sendable for Segments {
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()> {
        let payload = self.descriptor.payload();
        let len = payload.len();
        let ioslice = IoSlice::new(payload);

        let actual = socket.send_msg(
            self.descriptor.remote_address(),
            &[ioslice],
            self.segment_len,
            self.descriptor.ecn(),
        )?;

        debug_assert_eq!(len, actual);

        Ok(())
    }
}

impl Segments {
    /// Returns an iterator over segment sizes without consuming the Segments
    pub fn sizes(&self) -> SegmentSizesIter {
        SegmentSizesIter {
            remaining_len: self.descriptor.len(),
            segment_len: self.segment_len,
        }
    }
}

/// Iterator over segment sizes
pub struct SegmentSizesIter {
    remaining_len: u16,
    segment_len: u16,
}

impl Iterator for SegmentSizesIter {
    type Item = u16;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // If no remaining length, we're done
        if self.remaining_len == 0 {
            return None;
        }

        // If segment_len is 0, return entire remaining length
        if self.segment_len == 0 {
            let size = self.remaining_len;
            self.remaining_len = 0;
            return Some(size);
        }

        // If remaining exceeds segment length, return one segment
        if self.remaining_len > self.segment_len {
            self.remaining_len -= self.segment_len;
            return Some(self.segment_len);
        }

        // Return the last segment (smaller than segment_len)
        let size = self.remaining_len;
        self.remaining_len = 0;
        Some(size)
    }
}

impl IntoIterator for Segments {
    type Item = Filled;
    type IntoIter = SegmentsIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        SegmentsIter {
            descriptor: Some(self.descriptor),
            segment_len: self.segment_len,
        }
    }
}

/// An iterator over all of the filled segments in a packet
///
/// This is used for when the socket interface allows for receiving multiple packets
/// in a single syscall, e.g. GRO.
pub struct SegmentsIter {
    descriptor: Option<Filled>,
    segment_len: u16,
}

impl SegmentsIter {
    /// Creates an empty iterator that yields no segments.
    pub const fn empty() -> Self {
        Self {
            descriptor: None,
            segment_len: 0,
        }
    }
}

impl Iterator for SegmentsIter {
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
