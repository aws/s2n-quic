// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    free_list::FreeList,
    inner::{Half, Queue},
    probes,
};
use s2n_quic_core::{ensure, varint::VarInt};
use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

/// Trait for validating keys during dispatch
pub trait Key: 'static + Send {
    /// The request type used for validation
    type Request;

    /// Validates the provided request parameters against this key.
    ///
    /// Returns `Ok(())` if the request matches, or a specific error indicating
    /// which field mismatched.
    fn validate(&self, params: &Self::Request) -> Result<(), ValidationError>;
}

/// Indicates why a queue key validation failed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// The credential_id in the packet doesn't match the queue's owner
    CredentialMismatch,
    /// The stream_id in the packet doesn't match the queue's stream
    StreamIdMismatch,
    /// The packet matches the previous occupant of this queue (stale retransmit)
    Tombstone,
}

impl ValidationError {
    /// Returns the reset error code to send to the peer, or `None` for tombstone
    /// (which should be silently dropped).
    pub fn as_reset_code(self) -> Option<VarInt> {
        use crate::stream3::endpoint::reset_error;
        match self {
            Self::CredentialMismatch => Some(reset_error::CREDENTIAL_MISMATCH),
            Self::StreamIdMismatch => Some(reset_error::STREAM_ID_MISMATCH),
            Self::Tombstone => None,
        }
    }
}

impl Key for crate::credentials::Credentials {
    type Request = crate::credentials::Credentials;

    #[inline]
    fn validate(&self, params: &Self::Request) -> Result<(), ValidationError> {
        if self == params {
            Ok(())
        } else {
            Err(ValidationError::CredentialMismatch)
        }
    }
}

/// A pointer to a single descriptor in a group
///
/// Fundamentally, this is similar to something like `Arc<DescriptorInner>`. However,
/// unlike [`Arc`] which frees back to the global allocator, a Descriptor deallocates into
/// the backing [`FreeList`].
pub(super) struct Descriptor<S, C, Key> {
    ptr: NonNull<DescriptorInner<S, C, Key>>,
    phantom: PhantomData<DescriptorInner<S, C, Key>>,
}

impl<S: 'static, C: 'static, Key: 'static> Descriptor<S, C, Key> {
    #[inline]
    pub(super) fn new(ptr: NonNull<DescriptorInner<S, C, Key>>) -> Self {
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
    pub unsafe fn clone_for_sender(&self) -> Descriptor<S, C, Key> {
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
        self.ptr.as_ptr().addr()
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn queue_id(&self) -> VarInt {
        self.inner().id
    }

    /// Returns the peer's queue ID, or `None` if not yet observed.
    ///
    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn remote_queue_id(&self) -> Option<VarInt> {
        let v = self.inner().remote_queue_id.load(Ordering::Relaxed);
        VarInt::new(v).ok()
    }

    /// Stores the peer's queue ID with a relaxed store.
    ///
    /// Should only be called once per flow — guarded by the `HAS_OBSERVED` flag in the queue.
    ///
    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn set_remote_queue_id(&self, id: VarInt) {
        self.inner()
            .remote_queue_id
            .store(id.as_u64(), Ordering::Relaxed);
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn stream_queue(&self) -> &Queue<S> {
        &self.inner().stream
    }

    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn control_queue(&self) -> &Queue<C> {
        &self.inner().control
    }

    #[inline]
    fn inner(&self) -> &DescriptorInner<S, C, Key> {
        unsafe { self.ptr.as_ref() }
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be marked as free of receivers and the key must be uninitialized
    #[inline]
    pub unsafe fn init_key(&self, key: Key) {
        let inner = self.inner();
        (*inner.keys.get()).push(key);
    }

    /// # Safety
    ///
    /// * The [`Descriptor`] needs to be marked as free of receivers
    ///
    /// If `remote_queue_id` is `Some`, the value is stored immediately and both queue
    /// halves are marked as already observed (no dispatcher-side store needed).
    #[inline]
    pub unsafe fn into_receiver_pair(self, remote_queue_id: Option<VarInt>) -> (Self, Self) {
        let inner = self.inner();

        let has_remote_queue_id = remote_queue_id.is_some();
        if let Some(id) = remote_queue_id {
            inner.remote_queue_id.store(id.as_u64(), Ordering::Relaxed);
        } else {
            inner
                .remote_queue_id
                .store(REMOTE_QUEUE_ID_UNKNOWN, Ordering::Relaxed);
        }

        // open the queues back up for receiving
        inner
            .stream
            .open_receivers(&inner.control, has_remote_queue_id)
            .unwrap();

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
    pub unsafe fn drop_receiver(&self, half: Half)
    where
        Key: 'static,
    {
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

    /// Validate the request against the current key. If validation fails, checks
    /// the tombstone (previous occupant) to distinguish stale retransmits from
    /// genuine errors.
    ///
    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn validate(
        &self,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<(), ValidationError>
    where
        Key: super::descriptor::Key,
    {
        let inner = self.inner();
        let keys = &*inner.keys.get();
        let result = keys.current().validate(params);
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                if keys.tombstone_contains(params) {
                    return Err(ValidationError::Tombstone);
                }
                Err(e)
            }
        }
    }
}

unsafe impl<S: Send, C: Send, Key: Send> Send for Descriptor<S, C, Key> {}
unsafe impl<S: Sync, C: Sync, Key: Sync> Sync for Descriptor<S, C, Key> {}

/// Sentinel value indicating the remote queue ID is not yet known.
const REMOTE_QUEUE_ID_UNKNOWN: u64 = u64::MAX;

pub(super) struct DescriptorInner<S, C, Key> {
    id: VarInt,
    /// The peer's queue ID, written once by the dispatcher on first observation.
    /// Initialized to `u64::MAX` (unknown) and set via a relaxed store.
    remote_queue_id: AtomicU64,
    /// Ring buffer holding the current key and previous occupants (tombstones).
    /// The slot at `keys.current()` is the active key; all other initialized
    /// slots are tombstones for recognizing stale retransmissions.
    keys: UnsafeCell<KeyRing<Key>>,
    stream: Queue<S>,
    control: Queue<C>,
    /// A reference back to the free list
    free_list: Arc<dyn FreeList<S, C, Key>>,
    senders: AtomicUsize,
}

const KEY_RING_LEN: usize = 8;
const KEY_RING_INDEX_MASK: u16 = (KEY_RING_LEN - 1) as u16;

/// Ring buffer of keys: one current + N-1 tombstones. Power-of-2 sized for fast masking.
struct KeyRing<Key> {
    entries: [MaybeUninit<Key>; KEY_RING_LEN],
    /// Packed state: bits [0,3) = current index, bits [3,11) = init mask per slot.
    state: u16,
}

impl<Key> KeyRing<Key> {
    const LEN: usize = KEY_RING_LEN;

    fn new() -> Self {
        Self {
            entries: [const { MaybeUninit::uninit() }; KEY_RING_LEN],
            state: 0,
        }
    }

    fn init_bit(idx: usize) -> u16 {
        1u16 << (3 + idx)
    }

    fn is_init(&self, idx: usize) -> bool {
        self.state & Self::init_bit(idx) != 0
    }

    fn current_index(&self) -> usize {
        self.next_index().wrapping_sub(1) & KEY_RING_INDEX_MASK as usize
    }

    fn next_index(&self) -> usize {
        (self.state & KEY_RING_INDEX_MASK) as usize
    }

    /// Returns a reference to the current (active) key.
    fn current(&self) -> &Key {
        let idx = self.current_index();
        debug_assert!(self.is_init(idx));
        unsafe { self.entries[idx].assume_init_ref() }
    }

    fn push(&mut self, key: Key) {
        let idx = self.next_index();
        let bit = Self::init_bit(idx);

        if self.state & bit != 0 {
            unsafe { self.entries[idx].assume_init_drop() };
        }

        self.entries[idx] = MaybeUninit::new(key);
        self.state |= bit;
        let next = (idx + 1) & KEY_RING_INDEX_MASK as usize;
        self.state = (self.state & !KEY_RING_INDEX_MASK) | next as u16;
    }

    /// Check if any tombstone (non-current initialized slot) matches the params.
    fn tombstone_contains(&self, params: &<Key as super::descriptor::Key>::Request) -> bool
    where
        Key: super::descriptor::Key,
    {
        let current = self.current_index();
        for i in 0..Self::LEN {
            if i == current {
                continue;
            }
            if self.is_init(i) {
                let key = unsafe { self.entries[i].assume_init_ref() };
                if key.validate(params).is_ok() {
                    return true;
                }
            }
        }
        false
    }
}

impl<Key> Drop for KeyRing<Key> {
    fn drop(&mut self) {
        for i in 0..Self::LEN {
            if self.is_init(i) {
                unsafe { self.entries[i].assume_init_drop() };
            }
        }
    }
}

impl<S, C, Key> DescriptorInner<S, C, Key> {
    pub(super) fn new(id: VarInt, free_list: Arc<dyn FreeList<S, C, Key>>) -> Self {
        let stream = Queue::new(Half::Stream);
        let control = Queue::new(Half::Control);
        Self {
            id,
            remote_queue_id: AtomicU64::new(REMOTE_QUEUE_ID_UNKNOWN),
            keys: UnsafeCell::new(KeyRing::new()),
            stream,
            control,
            senders: AtomicUsize::new(0),
            free_list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct TestKey(u64);

    impl Key for TestKey {
        type Request = u64;

        fn validate(&self, params: &u64) -> Result<(), ValidationError> {
            if self.0 == *params {
                Ok(())
            } else {
                Err(ValidationError::StreamIdMismatch)
            }
        }
    }

    const LEN: usize = KeyRing::<TestKey>::LEN;

    #[test]
    fn empty_ring_tombstone_contains_nothing() {
        let ring = KeyRing::<TestKey>::new();
        assert!(!ring.tombstone_contains(&42));
    }

    #[test]
    fn current_key_accessible() {
        let mut ring = KeyRing::<TestKey>::new();
        ring.push(TestKey(7));
        assert_eq!(ring.current().0, 7);
    }

    #[test]
    fn previous_key_becomes_tombstone() {
        let mut ring = KeyRing::<TestKey>::new();
        ring.push(TestKey(1));
        ring.push(TestKey(2));
        assert_eq!(ring.current().0, 2);
        assert!(ring.tombstone_contains(&1));
    }

    #[test]
    fn current_is_not_tombstone() {
        let mut ring = KeyRing::<TestKey>::new();
        ring.push(TestKey(1));
        assert!(!ring.tombstone_contains(&1));
    }

    #[test]
    fn full_cycle_evicts_oldest() {
        let mut ring = KeyRing::<TestKey>::new();
        for i in 0..LEN as u64 + 1 {
            ring.push(TestKey(i));
        }
        // Current is LEN, tombstones are 1..LEN, oldest (0) evicted
        assert_eq!(ring.current().0, LEN as u64);
        assert!(!ring.tombstone_contains(&0));
        for i in 1..LEN as u64 {
            assert!(ring.tombstone_contains(&i), "should contain {i}");
        }
    }

    #[test]
    fn many_generations() {
        let mut ring = KeyRing::<TestKey>::new();
        for i in 0..LEN as u64 * 3 {
            ring.push(TestKey(i));
        }
        let last = LEN as u64 * 3 - 1;
        assert_eq!(ring.current().0, last);
        // LEN-1 tombstones visible
        for i in (last - LEN as u64 + 1)..last {
            assert!(ring.tombstone_contains(&i), "should contain {i}");
        }
        // Older ones evicted
        assert!(!ring.tombstone_contains(&(last - LEN as u64)));
    }

    #[test]
    fn drop_runs_cleanly() {
        let mut ring = KeyRing::<TestKey>::new();
        ring.push(TestKey(1));
        ring.push(TestKey(2));
        drop(ring);
    }

    #[test]
    fn drop_many_generations() {
        let mut ring = KeyRing::<TestKey>::new();
        for i in 0..LEN as u64 * 2 {
            ring.push(TestKey(i));
        }
        drop(ring);
    }
}
