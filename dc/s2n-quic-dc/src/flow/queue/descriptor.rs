// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    free_list::FreeList,
    inner::{Half, Queue},
    probes, queue_id,
};
use s2n_quic_core::{ensure, varint::VarInt};
use std::{
    cell::UnsafeCell,
    marker::PhantomData,
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
}

impl ValidationError {
    /// Returns the reset error code to send to the peer.
    pub fn as_reset_code(self) -> VarInt {
        use crate::stream::endpoint::error;
        match self {
            Self::CredentialMismatch => error::CREDENTIAL_MISMATCH,
            Self::StreamIdMismatch => error::STREAM_ID_MISMATCH,
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
    /// While allocated, `queue_id` is always initialized to a valid `VarInt`.
    #[inline]
    pub unsafe fn queue_id(&self) -> VarInt {
        let v = self.inner().queue_id.load(Ordering::Relaxed);
        debug_assert!(
            VarInt::new(v).is_ok(),
            "queue id should be initialized while allocated"
        );
        // SAFETY: callers must only invoke this for allocated descriptors, and
        // allocation initializes `queue_id` with a valid VarInt encoding.
        unsafe { VarInt::new_unchecked(v) }
    }

    /// Returns the queue ID if this descriptor is currently allocated.
    ///
    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated.
    #[inline]
    pub unsafe fn try_queue_id(&self) -> Option<VarInt> {
        let v = self.inner().queue_id.load(Ordering::Relaxed);
        VarInt::new(v).ok()
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
        debug_assert!((*inner.key.get()).is_none());
        *inner.key.get() = Some(key);
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

        let generation = {
            let next_generation = &mut *inner.next_generation.get();
            let current = *next_generation;
            *next_generation = current.wrapping_add(1);
            current
        };
        let queue_id = queue_id::encode(inner.id.as_u64() as usize, generation);
        inner.queue_id.store(queue_id.as_u64(), Ordering::Relaxed);

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
            .close_receiver(&inner.control, half, || {
                inner
                    .queue_id
                    .store(REMOTE_QUEUE_ID_UNKNOWN, Ordering::Relaxed);
                inner.clear_key();
            })
            .is_continue());

        probes::on_receiver_free(inner.id, half);

        let storage = inner.free_list.free(Descriptor {
            ptr: self.ptr,
            phantom: PhantomData,
        });
        drop(storage);
    }

    /// Validate the request against the current key.
    ///
    /// # Safety
    ///
    /// The caller needs to guarantee the [`Descriptor`] is still allocated and key
    /// access is synchronized by the queue mutex from the push path.
    #[inline]
    pub unsafe fn validate(
        &self,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<(), ValidationError>
    where
        Key: super::descriptor::Key,
    {
        let inner = self.inner();
        let key = (*inner.key.get())
            .as_ref()
            .expect("queue key should be initialized while allocated");
        key.validate(params)
    }
}

unsafe impl<S: Send, C: Send, Key: Send> Send for Descriptor<S, C, Key> {}
unsafe impl<S: Sync, C: Sync, Key: Sync> Sync for Descriptor<S, C, Key> {}

/// Sentinel value indicating the remote queue ID is not yet known.
const REMOTE_QUEUE_ID_UNKNOWN: u64 = u64::MAX;

pub(super) struct DescriptorInner<S, C, Key> {
    id: VarInt,
    queue_id: AtomicU64,
    /// The peer's queue ID, written once by the dispatcher on first observation.
    /// Initialized to `u64::MAX` (unknown) and set via a relaxed store.
    remote_queue_id: AtomicU64,
    /// Current allocation key.
    ///
    /// Access must be synchronized by holding the queue mutex so key reads and key
    /// clearing cannot race with queue allocation state transitions.
    key: UnsafeCell<Option<Key>>,
    next_generation: UnsafeCell<u64>,
    stream: Queue<S>,
    control: Queue<C>,
    /// A reference back to the free list
    free_list: Arc<dyn FreeList<S, C, Key>>,
    senders: AtomicUsize,
}

impl<S, C, Key> DescriptorInner<S, C, Key> {
    pub(super) fn new(index: usize, free_list: Arc<dyn FreeList<S, C, Key>>) -> Self {
        let stream = Queue::new(Half::Stream);
        let control = Queue::new(Half::Control);
        Self {
            id: VarInt::new(index as u64).unwrap(),
            queue_id: AtomicU64::new(REMOTE_QUEUE_ID_UNKNOWN),
            remote_queue_id: AtomicU64::new(REMOTE_QUEUE_ID_UNKNOWN),
            key: UnsafeCell::new(None),
            next_generation: UnsafeCell::new(0),
            stream,
            control,
            senders: AtomicUsize::new(0),
            free_list,
        }
    }

    #[inline]
    fn clear_key(&self) {
        unsafe { *self.key.get() = None };
    }
}

#[cfg(test)]
mod tests {
    use super::queue_id;

    #[test]
    fn queue_id_preserves_slot_bits() {
        let index = (1usize << queue_id::INDEX_BITS) - 1;
        let queue_id = queue_id::encode(index, 0);
        assert_eq!(queue_id::index(queue_id), index);
    }

    #[test]
    fn queue_id_preserves_generation_bits() {
        let index = 1234;
        let generation = queue_id::GENERATION_MASK;
        let queue_id = queue_id::encode(index, generation);
        assert_eq!(queue_id::generation(queue_id), generation);
        assert_eq!(queue_id::index(queue_id), index);
    }
}
