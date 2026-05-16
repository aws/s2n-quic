// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Send-safe sharded intrusive queue channel for normal async runtimes.
//!
//! The sender has no backpressure - it can always push a shard-local input batch to one of the
//! shards. The receiver drains one shard at a time, returning the shard-local storage value. The
//! receiver waker is stored in an [`AtomicWaker`] so it can be registered at any time, including
//! after senders have been cloned or moved to other threads.

use crate::intrusive;
use atomic_waker::AtomicWaker;
use core::{marker::PhantomData, task::Poll};
use sync::{lock, Arc, AtomicU64, AtomicUsize, Mutex, Ordering};

#[cfg(all(loom, test))]
mod sync {
    pub use loom::sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    };

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap()
    }
}

#[cfg(not(all(loom, test)))]
mod sync {
    pub use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    pub use parking_lot::{Mutex, MutexGuard};
    pub use std::sync::Arc;

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock()
    }
}

/// Shard-local storage used by the channel to accumulate sender input under the shard lock.
pub trait Storage<A: intrusive::Adapter>: Default {
    fn is_empty(&self) -> bool;
}

/// A value that can be appended into a particular [`Storage`] type.
pub trait Input<A: intrusive::Adapter, S: Storage<A>>: Sized {
    fn is_empty(&self) -> bool;
    fn append_to(self, storage: &mut S);
}

impl<A: intrusive::Adapter> Storage<A> for intrusive::List<A> {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<A: intrusive::Adapter> Input<A, intrusive::List<A>> for intrusive::List<A> {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        intrusive::List::is_empty(self)
    }

    #[inline(always)]
    fn append_to(mut self, storage: &mut intrusive::List<A>) {
        storage.append(&mut self);
    }
}

struct Shard<A: intrusive::Adapter, Q: Storage<A>> {
    is_open: bool,
    queue: Q,
    // `A` is only expressed through `Q: Storage<A>`, so keep an explicit marker on the shard to
    // preserve that type relationship while still letting auto-trait derivation come from `Q`.
    _marker: PhantomData<fn() -> A>,
}

struct Shared<A: intrusive::Adapter, Q: Storage<A>> {
    sender_count: AtomicUsize,
    next_sender_shard: AtomicUsize,
    sender_stride: usize,
    shard_mask: usize,
    occupancy: Box<[AtomicU64]>,
    /// Waker for the receiver task. Uses `AtomicWaker` so it can be registered at any time,
    /// even after senders have been cloned or sent to other threads. This removes the requirement
    /// from earlier designs that `Receiver::register` had to be called before any sender clone.
    recv_waker: AtomicWaker,
    shards: Box<[Mutex<Shard<A, Q>>]>,
}

impl<A: intrusive::Adapter, Q: Storage<A>> Shared<A, Q> {
    #[inline(always)]
    fn allocate_sender_shard(&self) -> usize {
        // Sender start positions intentionally wrap around the shard mask once there are more
        // senders than shards.
        self.next_sender_shard
            .fetch_add(self.sender_stride, Ordering::Relaxed)
            & self.shard_mask
    }

    #[inline(always)]
    fn occupancy_word_and_bit(shard: usize) -> (usize, u64) {
        // Map a shard index to its occupancy word and bit in the bitmap.
        let word = shard / u64::BITS as usize;
        let bit = 1 << (shard % u64::BITS as usize);
        (word, bit)
    }

    #[inline(always)]
    fn set_occupied(&self, shard: usize) {
        let (word, bit) = Self::occupancy_word_and_bit(shard);
        self.occupancy[word].fetch_or(bit, Ordering::Release);
    }

    #[inline(always)]
    fn wake_receiver(&self) {
        self.recv_waker.wake();
    }
}

#[inline(always)]
fn sender_stride(shard_count: usize) -> usize {
    // Start near half the shard count to spread consecutive senders apart, then force the result
    // to be odd. Odd values are coprime with power-of-two shard counts, so each sender walks every
    // shard before repeating.
    ((shard_count / 2).saturating_sub(1)) | 1
}

pub fn new<T>(
    shard_count: usize,
) -> (
    Sender<intrusive::EntryAdapter<T>>,
    Receiver<intrusive::EntryAdapter<T>>,
) {
    new_with_adapter::<intrusive::EntryAdapter<T>>(shard_count)
}

pub fn new_with_storage<T, Q>(
    shard_count: usize,
) -> (
    Sender<intrusive::EntryAdapter<T>, Q>,
    Receiver<intrusive::EntryAdapter<T>, Q>,
)
where
    Q: Storage<intrusive::EntryAdapter<T>>,
{
    new_with_adapter_and_storage::<intrusive::EntryAdapter<T>, Q>(shard_count)
}

/// Creates a sharded intrusive queue channel.
pub fn new_with_adapter<A: intrusive::Adapter>(shard_count: usize) -> (Sender<A>, Receiver<A>) {
    new_with_adapter_and_storage::<A, intrusive::List<A>>(shard_count)
}

pub fn new_with_adapter_and_storage<A, Q>(shard_count: usize) -> (Sender<A, Q>, Receiver<A, Q>)
where
    A: intrusive::Adapter,
    Q: Storage<A>,
{
    assert!(
        shard_count.is_power_of_two(),
        "shard count must be a power of two"
    );

    let occupancy_len = shard_count.div_ceil(u64::BITS as usize);
    let occupancy = (0..occupancy_len)
        .map(|_| AtomicU64::new(0))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let local_occupancy = vec![0; occupancy_len].into_boxed_slice();
    let shards = (0..shard_count)
        .map(|_| {
            Mutex::new(Shard {
                is_open: true,
                queue: Q::default(),
                // Keep the adapter/storage relationship on each shard even though the adapter is
                // only represented indirectly through the storage type.
                _marker: PhantomData,
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let sender_stride = sender_stride(shard_count);
    let shared = Arc::new(Shared {
        sender_count: AtomicUsize::new(1),
        next_sender_shard: AtomicUsize::new(0),
        sender_stride,
        shard_mask: shard_count - 1,
        occupancy,
        recv_waker: AtomicWaker::new(),
        shards,
    });

    let sender = Sender {
        next_shard: shared.allocate_sender_shard(),
        shared: shared.clone(),
    };
    let receiver = Receiver {
        next_shard: 0,
        local_occupancy,
        shared,
    };

    (sender, receiver)
}

pub struct Sender<A: intrusive::Adapter, Q: Storage<A> = intrusive::List<A>> {
    next_shard: usize,
    shared: Arc<Shared<A, Q>>,
}

impl<A: intrusive::Adapter, Q: Storage<A>> Clone for Sender<A, Q> {
    fn clone(&self) -> Self {
        self.shared.sender_count.fetch_add(1, Ordering::Relaxed);
        Self {
            next_shard: self.shared.allocate_sender_shard(),
            shared: self.shared.clone(),
        }
    }
}

impl<A: intrusive::Adapter, Q: Storage<A>> Drop for Sender<A, Q> {
    fn drop(&mut self) {
        if self.shared.sender_count.fetch_sub(1, Ordering::Release) == 1 {
            self.shared.wake_receiver();
        }
    }
}

impl<A: intrusive::Adapter, Q: Storage<A>> Sender<A, Q> {
    #[inline(always)]
    fn next_shard(&mut self) -> usize {
        let shard = self.next_shard;
        // The creation-time stride spreads senders out; each sender then walks adjacent shards to
        // avoid repeatedly colliding with other senders using the same stride.
        self.next_shard = (shard + 1) & self.shared.shard_mask;
        shard
    }

    pub fn send_batch<I: Input<A, Q>>(&mut self, batch: I) -> Result<(), I> {
        if batch.is_empty() {
            return Ok(());
        }

        let shard = self.next_shard();
        let mut queue = lock(&self.shared.shards[shard]);

        if !queue.is_open {
            return Err(batch);
        }

        let was_empty = <Q as Storage<A>>::is_empty(&queue.queue);
        batch.append_to(&mut queue.queue);
        drop(queue);

        if was_empty {
            self.shared.set_occupied(shard);
            self.shared.wake_receiver();
        }

        Ok(())
    }
}

impl<I, A, Q> super::super::UnboundedSender<I> for Sender<A, Q>
where
    A: intrusive::Adapter,
    Q: Storage<A>,
    I: Input<A, Q>,
{
    #[inline(always)]
    fn send(&mut self, batch: I) -> Result<(), I> {
        self.send_batch(batch)
    }
}

impl<I, A, Q> super::super::Sender<I> for Sender<A, Q>
where
    A: intrusive::Adapter,
    Q: Storage<A>,
    I: Input<A, Q>,
{
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        slot: &mut core::mem::MaybeUninit<I>,
    ) -> Poll<Result<(), ()>> {
        // SAFETY: the Sender trait requires callers to provide an initialized slot.
        let batch = unsafe { slot.assume_init_read() };
        match self.send_batch(batch) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(batch) => {
                slot.write(batch);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<A: intrusive::Adapter, Q: Storage<A> = intrusive::List<A>> {
    next_shard: usize,
    local_occupancy: Box<[u64]>,
    shared: Arc<Shared<A, Q>>,
}

impl<A: intrusive::Adapter, Q: Storage<A>> Drop for Receiver<A, Q> {
    fn drop(&mut self) {
        for shard in self.shared.shards.iter() {
            lock(shard).is_open = false;
        }
    }
}

impl<A: intrusive::Adapter, Q: Storage<A>> Receiver<A, Q> {
    /// Registers the receiver waker.
    ///
    /// Uses [`AtomicWaker`] internally, so this may be called at any time — including after senders
    /// have been cloned or moved to other threads — without violating memory-safety. Callers
    /// should still register as early as possible (e.g. on first task poll) so that sends that
    /// arrive before registration are not missed.
    pub fn register(&self, waker: &core::task::Waker) {
        self.shared.recv_waker.register(waker);
    }

    #[inline(always)]
    fn try_swap(&mut self, batch: &mut Q) -> TrySwap {
        // Only consume one occupied bit per receive attempt so stale occupancy bookkeeping stays
        // visible to debug builds instead of being hidden by looking for another ready shard.
        if let Some(shard) = self.next_occupied() {
            let mut queue = lock(&self.shared.shards[shard]);
            debug_assert!(
                !<Q as Storage<A>>::is_empty(&queue.queue),
                "occupancy bit set for an empty shard"
            );

            assert!(
                <Q as Storage<A>>::is_empty(batch),
                "poll_swap requires the caller to provide empty storage"
            );
            core::mem::swap(batch, &mut queue.queue);
            return TrySwap::Ready;
        }

        TrySwap::Empty
    }

    /// Swaps the next ready shard into `batch`.
    ///
    /// `batch` must be empty before calling this method. On `Ready(Some(()))`, `batch` contains
    /// the drained shard contents and the receiver has taken ownership of the empty storage value
    /// that was previously in `batch`.
    #[inline(always)]
    pub fn poll_swap(
        &mut self,
        cx: &mut core::task::Context<'_>,
        batch: &mut Q,
    ) -> Poll<Option<()>> {
        // Register the waker before checking for items so we cannot miss a concurrent send
        // between the occupancy check and returning Poll::Pending.
        self.shared.recv_waker.register(cx.waker());

        if let TrySwap::Ready = self.try_swap(batch) {
            return Poll::Ready(Some(()));
        }

        if self.shared.sender_count.load(Ordering::Acquire) == 0 {
            if let TrySwap::Ready = self.try_swap(batch) {
                return Poll::Ready(Some(()));
            }

            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn next_occupied(&mut self) -> Option<usize> {
        let word_count = self.local_occupancy.len();
        debug_assert!(word_count.is_power_of_two());
        let start_shard = self.next_shard;
        let start_word = start_shard / u64::BITS as usize;
        let start_bit = start_shard % u64::BITS as usize;

        if let Some(shard) = self.next_occupied_in_word(
            start_word,
            self.valid_word_mask(start_word) & (!0 << start_bit),
        ) {
            return Some(shard);
        }

        for offset in 1..word_count {
            let word = (start_word + offset) & (word_count - 1);
            if let Some(shard) = self.next_occupied_in_word(word, self.valid_word_mask(word)) {
                return Some(shard);
            }
        }

        if start_bit > 0 {
            let mask = self.valid_word_mask(start_word) & ((1 << start_bit) - 1);
            if let Some(shard) = self.next_occupied_in_word(start_word, mask) {
                return Some(shard);
            }
        }

        self.next_shard = 0;
        None
    }

    #[inline(always)]
    fn next_occupied_in_word(&mut self, word: usize, mask: u64) -> Option<usize> {
        if mask == 0 {
            return None;
        }

        let mut bits = self.local_occupancy[word] & mask;
        if bits == 0 {
            // The swap only needs Acquire ordering: senders publish queue writes with Release
            // ordering before setting the occupancy bit, and the receiver does not publish data
            // when clearing it.
            self.local_occupancy[word] |= self.shared.occupancy[word].swap(0, Ordering::Acquire);
            bits = self.local_occupancy[word] & mask;
        }
        if bits == 0 {
            return None;
        }

        let bit = bits.trailing_zeros() as usize;
        self.local_occupancy[word] &= !(1 << bit);
        let shard = word * u64::BITS as usize + bit;
        self.next_shard = (shard + 1) & self.shared.shard_mask;
        Some(shard)
    }

    #[inline(always)]
    fn valid_word_mask(&self, word: usize) -> u64 {
        let word_count = self.local_occupancy.len();
        if word + 1 != word_count {
            return !0;
        }

        let first_shard = word * u64::BITS as usize;
        debug_assert!(first_shard < self.shared.shards.len());
        let valid_bits = self.shared.shards.len() - first_shard;
        if valid_bits == u64::BITS as usize {
            !0
        } else {
            (1 << valid_bits) - 1
        }
    }
}

impl<A: intrusive::Adapter> super::super::Receiver<intrusive::List<A>>
    for Receiver<A, intrusive::List<A>>
{
    #[inline(always)]
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> Poll<Option<intrusive::List<A>>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        let mut batch = intrusive::List::new();

        match self.poll_swap(cx, &mut batch) {
            Poll::Ready(Some(())) => {
                budget.consume();
                Poll::Ready(Some(batch))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}

enum TrySwap {
    Ready,
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        intrusive::{Entry, Queue},
        socket::channel::{Budget, Receiver as _, UnboundedSender as _},
    };
    use core::task::Poll;

    fn noop_cx() -> core::task::Context<'static> {
        let waker = s2n_quic_core::task::waker::noop();
        let waker_ref = Box::leak(Box::new(waker));
        core::task::Context::from_waker(waker_ref)
    }

    fn register<A: intrusive::Adapter, Q: Storage<A>>(rx: &mut Receiver<A, Q>) {
        let waker = s2n_quic_core::task::waker::noop();
        rx.register(&waker);
    }

    fn list(values: impl IntoIterator<Item = u32>) -> Queue<u32> {
        let mut list = Queue::new();
        for value in values {
            list.push_back(Entry::new(value));
        }
        list
    }

    fn values(list: &Queue<u32>) -> Vec<u32> {
        list.iter().copied().collect()
    }

    #[derive(Debug, Default)]
    struct SplitQueue {
        // Records which receiver-provided replacement slot is currently backing the shard.
        slot_id: usize,
        even: Queue<u32>,
        odd: Queue<u32>,
    }

    impl SplitQueue {
        fn new(slot_id: usize) -> Self {
            Self {
                slot_id,
                even: Queue::new(),
                odd: Queue::new(),
            }
        }
    }

    impl Storage<intrusive::EntryAdapter<u32>> for SplitQueue {
        fn is_empty(&self) -> bool {
            self.even.is_empty() && self.odd.is_empty()
        }
    }

    impl Input<intrusive::EntryAdapter<u32>, SplitQueue> for Queue<u32> {
        fn is_empty(&self) -> bool {
            Queue::is_empty(self)
        }

        fn append_to(mut self, storage: &mut SplitQueue) {
            while let Some(entry) = self.pop_front() {
                if *entry % 2 == 0 {
                    storage.even.push_back(entry);
                } else {
                    storage.odd.push_back(entry);
                }
            }
        }
    }

    fn split_queue(values: impl IntoIterator<Item = u32>) -> Queue<u32> {
        let mut queue = Queue::new();
        for value in values {
            queue.push_back(Entry::new(value));
        }
        queue
    }

    #[derive(Debug, Default)]
    struct VecInputSplitQueue {
        even: Queue<u32>,
        odd: Queue<u32>,
    }

    impl Storage<intrusive::EntryAdapter<u32>> for VecInputSplitQueue {
        fn is_empty(&self) -> bool {
            self.even.is_empty() && self.odd.is_empty()
        }
    }

    impl Input<intrusive::EntryAdapter<u32>, VecInputSplitQueue> for Vec<Entry<u32>> {
        fn is_empty(&self) -> bool {
            Vec::is_empty(self)
        }

        fn append_to(self, storage: &mut VecInputSplitQueue) {
            for entry in self {
                if *entry % 2 == 0 {
                    storage.even.push_back(entry);
                } else {
                    storage.odd.push_back(entry);
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "shard count must be a power of two")]
    fn rejects_non_power_of_two_shards() {
        let _ = new::<u32>(3);
    }

    #[test]
    fn drains_entire_shard() {
        let (mut tx, mut rx) = new::<u32>(1);
        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);
        register(&mut rx);

        assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));

        tx.send(list([1, 2, 3])).unwrap();

        let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx, &mut budget) else {
            panic!("expected drained list");
        };
        assert_eq!(values(&list), vec![1, 2, 3]);

        assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));
    }

    #[test]
    fn sender_creation_selects_initial_shard() {
        let (mut tx0, mut rx) = new::<u32>(4);
        register(&mut rx);
        let mut tx1 = tx0.clone();
        let mut tx2 = tx0.clone();
        let mut tx3 = tx0.clone();
        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));

        tx3.send(list([3])).unwrap();
        tx2.send(list([2])).unwrap();
        tx1.send(list([1])).unwrap();
        tx0.send(list([0])).unwrap();

        let mut received = vec![];
        for _ in 0..4 {
            let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx, &mut budget) else {
                panic!("expected drained list");
            };
            assert_eq!(list.len(), 1);
            received.push(*list.front().unwrap());
        }

        assert_eq!(received, vec![0, 1, 2, 3]);
    }

    #[test]
    fn sender_round_robins_locally_by_one() {
        let (mut tx, mut rx) = new::<u32>(4);
        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);
        register(&mut rx);

        for value in 0..4 {
            tx.send(list([value])).unwrap();
        }

        for expected in 0..4 {
            let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx, &mut budget) else {
                panic!("expected drained list");
            };
            assert_eq!(values(&list), vec![expected]);
        }
    }

    #[test]
    fn custom_storage_appends_per_category() {
        let (mut tx, mut rx) = new_with_storage::<u32, SplitQueue>(1);
        let mut cx = noop_cx();
        register(&mut rx);

        tx.send(split_queue([1, 2])).unwrap();

        let mut batch = SplitQueue::new(0);

        let Poll::Ready(Some(())) = rx.poll_swap(&mut cx, &mut batch) else {
            panic!("expected drained split queue");
        };

        assert_eq!(batch.slot_id, 0);
        assert_eq!(values(&batch.even), vec![2]);
        assert_eq!(values(&batch.odd), vec![1]);

        tx.send(split_queue([5, 6])).unwrap();

        batch = SplitQueue::new(1);

        let Poll::Ready(Some(())) = rx.poll_swap(&mut cx, &mut batch) else {
            panic!("expected drained split queue");
        };

        assert_eq!(batch.slot_id, 0);
        assert_eq!(values(&batch.even), vec![6]);
        assert_eq!(values(&batch.odd), vec![5]);

        tx.send(split_queue([7, 8])).unwrap();

        batch = SplitQueue::new(2);

        let Poll::Ready(Some(())) = rx.poll_swap(&mut cx, &mut batch) else {
            panic!("expected drained split queue");
        };

        assert_eq!(batch.slot_id, 1);
        assert_eq!(values(&batch.even), vec![8]);
        assert_eq!(values(&batch.odd), vec![7]);
    }

    #[test]
    fn custom_storage_accepts_distinct_input_type() {
        let (mut tx, mut rx) = new_with_storage::<u32, VecInputSplitQueue>(1);
        let mut cx = noop_cx();
        register(&mut rx);

        tx.send(vec![Entry::new(1), Entry::new(2), Entry::new(3)])
            .unwrap();

        let mut batch = VecInputSplitQueue::default();

        let Poll::Ready(Some(())) = rx.poll_swap(&mut cx, &mut batch) else {
            panic!("expected drained vec split queue");
        };

        assert_eq!(values(&batch.even), vec![2]);
        assert_eq!(values(&batch.odd), vec![1, 3]);
    }

    #[test]
    fn sender_drop_closes_receiver() {
        let (tx, mut rx) = new::<u32>(2);
        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        assert!(matches!(rx.poll_recv(&mut cx, &mut budget), Poll::Pending));
        drop(tx);
        assert!(matches!(
            rx.poll_recv(&mut cx, &mut budget),
            Poll::Ready(None)
        ));
    }

    #[test]
    fn next_occupied_wraps_once() {
        let (_tx, mut rx) = new::<u32>(128);

        // Start in word 1 so the scan first finds a later shard in that word, then wraps back to
        // word 0.
        rx.next_shard = 65;
        rx.local_occupancy[1] = 1 << 6; // shard 70
        rx.local_occupancy[0] = 1 << 2; // shard 2

        assert_eq!(rx.next_occupied(), Some(70));
        assert_eq!(rx.next_occupied(), Some(2));
        assert_eq!(rx.next_occupied(), None);
        assert_eq!(rx.next_shard, 0);
    }

    #[test]
    fn next_occupied_respects_masks() {
        let (_tx, mut rx) = new::<u32>(8);

        rx.local_occupancy[0] = 1;
        assert_eq!(rx.next_occupied_in_word(0, 0), None);
        assert_eq!(rx.local_occupancy[0], 1);

        assert_eq!(rx.valid_word_mask(0), 0xff);
        // Bit 9 is outside the 8-shard range and should be ignored by the valid word mask.
        rx.local_occupancy[0] = (1 << 9) | (1 << 7);
        assert_eq!(rx.next_occupied(), Some(7));
        assert_eq!(rx.next_occupied(), None);
    }

    #[test]
    #[should_panic(expected = "occupancy bit set for an empty shard")]
    fn stale_occupancy_bit_panics() {
        let (_tx, mut rx) = new::<u32>(1);
        let mut cx = noop_cx();
        let mut budget = Budget::new(usize::MAX);

        rx.local_occupancy[0] = 1;

        let _ = rx.poll_recv(&mut cx, &mut budget);
    }

    #[test]
    fn receiver_drop_closes_sender() {
        let (mut tx, mut rx) = new::<u32>(2);
        register(&mut rx);
        drop(rx);

        assert_eq!(tx.send(list([1])).unwrap_err().len(), 1);
    }

    #[test]
    fn loom_concurrent_send_recv() {
        use crate::testing::loom;

        loom::model(|| {
            let (mut tx0, rx) = new::<u32>(2);
            let registered =
                loom::sync::Arc::new((loom::sync::Mutex::new(false), loom::sync::Condvar::new()));
            let registered_rx = registered.clone();

            let receiver = loom::thread::spawn(move || {
                loom::future::block_on(async move {
                    let mut rx = rx;
                    let mut received = vec![];
                    let mut budget = Budget::new(usize::MAX);

                    core::future::poll_fn(|cx| {
                        rx.register(cx.waker());
                        let (registered, cvar) = &*registered_rx;
                        *registered.lock().unwrap() = true;
                        cvar.notify_one();
                        Poll::Ready(())
                    })
                    .await;

                    loop {
                        let item = core::future::poll_fn(|cx| rx.poll_recv(cx, &mut budget)).await;

                        match item {
                            Some(list) => received.extend(values(&list)),
                            None => break,
                        }
                    }

                    received.sort_unstable();
                    assert_eq!(received, vec![1, 2]);
                });
            });

            let (registered, cvar) = &*registered;
            let mut is_registered = registered.lock().unwrap();
            while !*is_registered {
                is_registered = cvar.wait(is_registered).unwrap();
            }

            let mut tx1 = tx0.clone();

            let a = loom::thread::spawn(move || tx0.send(list([1])).unwrap());
            let b = loom::thread::spawn(move || tx1.send(list([2])).unwrap());

            a.join().unwrap();
            b.join().unwrap();
            receiver.join().unwrap();
        });
    }
}
