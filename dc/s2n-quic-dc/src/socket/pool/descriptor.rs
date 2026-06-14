// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator,
    intrusive::{self, Links},
    msg::{self, addr::Addr, cmsg},
    socket::channel::intrusive::{sync, unsync},
    tracing::trace,
};
use core::fmt;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    alloc::Layout,
    io::{IoSlice, IoSliceMut},
    ptr::NonNull,
    rc,
    sync::atomic::{AtomicUsize, Ordering},
};

// ── Recycler trait ────────────────────────────────────────────────────────

/// A strategy for returning a dropped descriptor back to a pool instead of
/// deallocating it.
///
/// # Safety
///
/// Implementors must ensure that `try_push` either transfers ownership of
/// `recycled` to the backing channel (returning `Ok`) or returns it in `Err`
/// so the caller can arrange deallocation via [`Recycled::drop`].
pub unsafe trait Recycler: Sized + 'static {
    /// Attempt to push `recycled` back into the recycling channel.
    ///
    /// Returns `Ok(())` when the descriptor is now owned by the channel.
    /// Returns `Err(recycled)` when the channel is closed; the caller must
    /// then let `Err(recycled)` drop so that [`Recycled::drop`] frees memory.
    ///
    /// # Safety
    ///
    /// The refcount inside `recycled` must already have been reset to 1 by
    /// the caller before invoking `try_push`.
    unsafe fn try_push(&self, recycled: Recycled<Self>) -> Result<(), Recycled<Self>>;
}

// ── SyncRecycler ──────────────────────────────────────────────────────────

/// Sync (mutex-guarded) recycler backed by an [`sync::AdapterShared`] channel.
///
/// Suitable for descriptors that may be dropped on a different thread from
/// the one that owns the recv socket (e.g. dispatch workers).
pub struct SyncRecycler(
    pub(crate) std::sync::Weak<sync::AdapterShared<RecycleAdapter<SyncRecycler>>>,
);

// SAFETY: `Weak<AdapterShared<…>>` is `Send + Sync`.
unsafe impl Send for SyncRecycler {}
unsafe impl Sync for SyncRecycler {}

impl Clone for SyncRecycler {
    fn clone(&self) -> Self {
        SyncRecycler(self.0.clone())
    }
}

unsafe impl Recycler for SyncRecycler {
    #[inline]
    unsafe fn try_push(&self, recycled: Recycled<Self>) -> Result<(), Recycled<Self>> {
        if let Some(shared) = self.0.upgrade() {
            shared.push(recycled)
        } else {
            Err(recycled)
        }
    }
}

// ── UnsyncRecycler ────────────────────────────────────────────────────────

/// Unsync (lock-free) recycler backed by an [`unsync::Shared`] channel.
///
/// Suitable for descriptors that are always dropped on the same thread that
/// allocated them (e.g. assembled send segments inside a `!Send` task).
/// Using this instead of [`SyncRecycler`] avoids a mutex acquisition on every
/// recycle/drain cycle.
pub struct UnsyncRecycler(pub(crate) rc::Weak<unsync::Shared<RecycleAdapter<UnsyncRecycler>>>);

// `rc::Weak` is `!Send + !Sync`, so `UnsyncRecycler` is automatically `!Send + !Sync`.

impl Clone for UnsyncRecycler {
    fn clone(&self) -> Self {
        UnsyncRecycler(self.0.clone())
    }
}

unsafe impl Recycler for UnsyncRecycler {
    #[inline]
    unsafe fn try_push(&self, recycled: Recycled<Self>) -> Result<(), Recycled<Self>> {
        if let Some(shared) = self.0.upgrade() {
            // SAFETY: `UnsyncRecycler` is `!Send`; all access is on the same thread.
            shared.push_recycled(recycled)
        } else {
            Err(recycled)
        }
    }
}

// ── Backward-compat alias ────────────────────────────────────────────────

/// Legacy alias kept so existing call sites compile without changes.
///
/// `SyncRecycler` replaced the old `Weak<AdapterShared<RecycleAdapter>>` type.
pub type WeakRecycleSender = SyncRecycler;

/// A descriptor in the recycling pipeline. Deallocates on drop.
///
/// This wrapper ensures that if a recycling queue or list is dropped
/// (e.g. on shutdown), the underlying allocation is properly freed.
/// To reuse a recycled descriptor, call [`into_descriptor`](Self::into_descriptor).
pub struct Recycled<R: Recycler = SyncRecycler>(Descriptor<R>);

impl<R: Recycler> Recycled<R> {
    pub fn into_descriptor(self) -> Descriptor<R> {
        let desc = Descriptor { ptr: self.0.ptr };
        core::mem::forget(self);
        desc
    }
}

impl<R: Recycler> Drop for Recycled<R> {
    fn drop(&mut self) {
        unsafe { self.0.dealloc() };
    }
}

// SAFETY: `Recycled<SyncRecycler>` is safe to send between threads because
// `SyncRecycler: Send` and the backing allocation is refcounted with atomics.
unsafe impl<R: Recycler + Send> Send for Recycled<R> {}
unsafe impl<R: Recycler + Sync> Sync for Recycled<R> {}

/// Intrusive adapter for recycled descriptors.
///
/// Uses `Recycled` as the pointer type so that dropping any `List<RecycleAdapter>`
/// (in the sync channel, local pool, etc.) properly deallocates all remaining
/// descriptors.
pub struct RecycleAdapter<R: Recycler = SyncRecycler>(core::marker::PhantomData<R>);

impl<R: Recycler> intrusive::Adapter for RecycleAdapter<R> {
    type Value = Header<R>;
    type Target = Header<R>;
    type Pointer = Recycled<R>;

    unsafe fn links(value: *mut Self::Value) -> *mut Links {
        core::ptr::addr_of_mut!((*value).links)
    }

    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
        value
    }

    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
        ptr.0.ptr.as_ptr()
    }

    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
        let raw = ptr.0.ptr.as_ptr();
        core::mem::forget(ptr);
        raw
    }

    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
        Recycled(Descriptor {
            ptr: NonNull::new_unchecked(ptr),
        })
    }
}

/// A pointer to a single descriptor
///
/// Each descriptor owns a contiguous allocation containing:
/// `[Header | Addr | payload bytes]`
///
/// Reference counting allows splitting a filled descriptor into multiple
/// segments (e.g., for GRO). When the last reference is dropped, the entire
/// allocation is freed through [`allocator::packet::dealloc`].
pub struct Descriptor<R: Recycler = SyncRecycler> {
    ptr: NonNull<Header<R>>,
}

impl<R: Recycler> Descriptor<R> {
    /// Allocates a new descriptor with the given payload `capacity`.
    ///
    /// Returns `None` if the packet subheap is exhausted.
    #[inline]
    fn alloc(capacity: u16, recycler: Option<R>) -> Option<Self> {
        let (layout, addr_offset, _payload_offset) = Header::<R>::layout(capacity);

        let ptr = allocator::packet::alloc(layout)?;

        unsafe {
            let base = ptr.as_ptr();

            // Payload is uninitialized - callers must fill before reading

            // Initialize the Header at the start of the allocation
            let inner_ptr = base.cast::<Header<R>>();
            inner_ptr.write(Header {
                capacity,
                references: AtomicUsize::new(1),
                recycler,
                links: Links::new(),
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
    fn inner(&self) -> &Header<R> {
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
    unsafe fn into_filled(self, len: u16, ecn: ExplicitCongestionNotification) -> Filled<R> {
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

        // Try to recycle instead of deallocating
        if let Some(recycler) = &inner.recycler {
            inner.references.store(1, Ordering::Relaxed);
            // Safety: recycler is a reference into the Header allocation; the
            // allocation remains alive (owned by `recycled` below) for the
            // duration of `try_push`.
            let recycler_ptr: *const R = recycler;
            let recycled = Recycled(Descriptor { ptr: self.ptr });
            match (*recycler_ptr).try_push(recycled) {
                Ok(()) => {
                    trace!(recycle_desc = ?self.ptr, state = %"filled");
                }
                Err(_recycled) => {
                    // Channel closed — Recycled::drop will dealloc
                }
            }
            return;
        }

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
        let capacity = inner.capacity;

        // Drop the recycler before freeing memory
        let header_ptr = self.ptr.as_ptr();
        core::ptr::drop_in_place(core::ptr::addr_of_mut!((*header_ptr).recycler));

        const { assert!(!core::mem::needs_drop::<Addr>()) };
        const { assert!(!core::mem::needs_drop::<Links>()) };

        let (layout, _addr_offset, _payload_offset) = Header::<R>::layout(capacity);
        let base = self.ptr.cast::<u8>();
        allocator::packet::dealloc(base, layout);
    }
}

// SAFETY: The backing allocation uses `AtomicUsize` for refcounting, making
// it safe to transfer between threads when R itself is Send.
unsafe impl<R: Recycler + Send> Send for Descriptor<R> {}
// SAFETY: All mutable access is guarded by the atomic refcount; shared
// references only read immutable fields.
unsafe impl<R: Recycler + Sync> Sync for Descriptor<R> {}

pub struct Header<R: Recycler = SyncRecycler> {
    /// The maximum capacity for this descriptor
    capacity: u16,
    /// The number of active references for this descriptor.
    ///
    /// This refcount allows for splitting descriptors into multiple segments
    /// and then correctly freeing the descriptor once the last segment is dropped.
    references: AtomicUsize,
    /// Recycler for returning this descriptor to the pool instead of deallocating.
    /// None for descriptors that are always deallocated (never recycled).
    recycler: Option<R>,
    /// Intrusive links for queue membership during recycling.
    /// Only active when the descriptor is in a recycling queue.
    links: Links,
}

impl<R: Recycler> Header<R> {
    /// Computes the layout for the contiguous allocation:
    /// `[Header | Addr | payload bytes]`
    ///
    /// Returns `(layout, addr_offset, payload_offset)`.
    #[inline]
    const fn layout(capacity: u16) -> (Layout, usize, usize) {
        let inner = Layout::new::<Header<R>>();
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
pub struct Unfilled<R: Recycler = SyncRecycler> {
    /// The inner raw descriptor.
    ///
    /// This needs to be an [`Option`] to allow for both consuming the descriptor
    /// into a [`Filled`] after receiving a packet or dropping the [`Unfilled`] and
    /// releasing it back into the packet allocator.
    desc: Option<Descriptor<R>>,
}

impl<R: Recycler> fmt::Debug for Unfilled<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Unfilled").finish()
    }
}

impl<R: Recycler> Unfilled<R> {
    /// Allocates a new unfilled packet with the given payload capacity.
    ///
    /// Returns `None` if the packet subheap is exhausted.
    #[inline]
    pub fn new(capacity: u16) -> Option<Self> {
        let desc = Descriptor::alloc(capacity, None)?;
        Some(Self { desc: Some(desc) })
    }

    /// Allocates a new unfilled packet with a recycler attached.
    ///
    /// When this descriptor is eventually dropped (after being filled and consumed),
    /// it will be pushed back to the recycling channel instead of being deallocated.
    #[inline]
    pub fn new_with_recycler(capacity: u16, recycler: R) -> Option<Self> {
        let desc = Descriptor::alloc(capacity, Some(recycler))?;
        Some(Self { desc: Some(desc) })
    }

    /// Wraps a recycled descriptor back into an Unfilled.
    ///
    /// The descriptor's Header (capacity, recycler, etc.) is still valid from
    /// the original allocation.
    #[inline]
    pub(crate) fn from_recycled(desc: Descriptor<R>) -> Self {
        Self { desc: Some(desc) }
    }

    /// Converts this unfilled descriptor into the recycled form for direct pool priming.
    ///
    /// This bypasses the recycling channel so the descriptor lands straight in the
    /// caller's local LIFO list, ready to be handed out on the very first
    /// [`alloc_or_reuse`](crate::socket::pool::UnsyncReusePool::alloc_or_reuse) call.
    #[inline]
    pub(crate) fn into_recycled(mut self) -> Recycled<R> {
        let desc = self.desc.take().expect("valid state");
        Recycled(desc)
    }

    /// Fills the packet with the given callback, if the callback is successful
    #[inline]
    pub fn fill_with<F, E>(mut self, f: F) -> Result<Segments<R>, (Self, E)>
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

impl<R: Recycler> Drop for Unfilled<R> {
    #[inline]
    fn drop(&mut self) {
        if let Some(desc) = self.desc.take() {
            unsafe {
                // Try to recycle instead of deallocating
                let inner = desc.inner();
                if let Some(recycler) = &inner.recycler {
                    inner.references.store(1, Ordering::Relaxed);
                    // Safety: `recycler` borrows from `inner`; we capture a raw
                    // pointer so the borrow ends before moving `desc`.
                    let recycler_ptr: *const R = recycler;
                    let recycled = Recycled(desc);
                    match (*recycler_ptr).try_push(recycled) {
                        Ok(()) => {
                            trace!(recycle = %"unfilled");
                            return;
                        }
                        Err(_recycled) => {
                            // Channel closed — Recycled::drop will dealloc
                            return;
                        }
                    }
                }
                // SAFETY: the descriptor is in the `unfilled` state and no longer used
                desc.drop_unfilled();
            }
        }
    }
}

/// A filled packet
pub struct Filled<R: Recycler = SyncRecycler> {
    /// The raw descriptor
    desc: Descriptor<R>,
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

impl<R: Recycler> fmt::Debug for Filled<R> {
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

impl<R: Recycler> Filled<R> {
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

impl<R: Recycler> core::ops::Deref for Filled<R> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.payload()
    }
}

impl<R: Recycler> core::ops::DerefMut for Filled<R> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.payload_mut()
    }
}

impl<R: Recycler> s2n_quic_core::buffer::reader::Storage for Filled<R> {
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

impl<R: Recycler> Filled<R> {
    /// Creates a deep copy of this filled descriptor by allocating a new buffer
    /// and copying the payload bytes. This is safe because the copy gets its own
    /// exclusive memory region.
    pub fn deep_copy(&self) -> Option<Self> {
        let Some(unfilled) = Unfilled::<R>::new(self.len) else {
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

impl<R: Recycler> Drop for Filled<R> {
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

pub struct Segments<R: Recycler = SyncRecycler> {
    descriptor: Filled<R>,
    segment_len: u16,
}

impl<R: Recycler> fmt::Debug for Segments<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segments")
            .field("payload_len", &self.descriptor.len())
            .field("segment_len", &self.segment_len)
            .finish()
    }
}

impl<R: Recycler> Segments<R> {
    pub fn new(descriptor: Filled<R>, segment_len: u16) -> Self {
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

    pub fn segment_count(&self) -> u16 {
        if self.segment_len == 0 {
            return 1;
        }
        self.descriptor.len().div_ceil(self.segment_len)
    }

    pub fn take_filled(self) -> Filled<R> {
        self.descriptor
    }

    pub fn send_with<F, Ret>(&self, f: F) -> Ret
    where
        F: FnOnce(&Addr, ExplicitCongestionNotification, &[IoSlice]) -> Ret,
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

impl<R: Recycler> crate::socket::channel::ByteCost for Segments<R> {
    fn byte_cost(&self) -> u64 {
        self.descriptor.len() as u64
    }
}

impl<R: Recycler> crate::socket::channel::Sendable for Segments<R> {
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

impl<R: Recycler> Segments<R> {
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

impl<R: Recycler> IntoIterator for Segments<R> {
    type Item = Filled<R>;
    type IntoIter = SegmentsIter<R>;

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
pub struct SegmentsIter<R: Recycler = SyncRecycler> {
    descriptor: Option<Filled<R>>,
    segment_len: u16,
}

impl<R: Recycler> SegmentsIter<R> {
    /// Creates an empty iterator that yields no segments.
    pub const fn empty() -> Self {
        Self {
            descriptor: None,
            segment_len: 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.descriptor.is_none()
    }
}

impl<R: Recycler> Iterator for SegmentsIter<R> {
    type Item = Filled<R>;

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
