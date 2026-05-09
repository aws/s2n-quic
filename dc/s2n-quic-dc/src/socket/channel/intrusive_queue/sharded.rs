// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Send-safe sharded intrusive queue channel for normal async runtimes.
//!
//! The sender has no backpressure - it can always push lists to one of the shards. The receiver
//! drains one shard at a time, returning the entire list. Receivers are expected to register their
//! waker immediately after channel creation and before cloning or exposing senders.

use crate::intrusive_queue;
use core::{
    cell::UnsafeCell,
    task::{Poll, Waker},
};
use sync::{lock, Arc, AtomicBool, AtomicU64, AtomicUsize, Mutex, Ordering};

#[cfg(all(loom, test))]
mod sync {
    pub use loom::sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    };

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap()
    }
}

#[cfg(not(all(loom, test)))]
mod sync {
    pub use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    pub use parking_lot::{Mutex, MutexGuard};
    pub use std::sync::Arc;

    #[inline(always)]
    pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock()
    }
}

struct Shard<A: intrusive_queue::Adapter> {
    is_open: bool,
    queue: intrusive_queue::List<A>,
}

struct Shared<A: intrusive_queue::Adapter> {
    sender_count: AtomicUsize,
    next_sender_shard: AtomicUsize,
    sender_stride: usize,
    shard_mask: usize,
    occupancy: Box<[AtomicU64]>,
    waker_registered: AtomicBool,
    // Initialized to a noop waker and updated by the receiver before senders are exposed.
    recv_waker: UnsafeCell<Waker>,
    shards: Box<[Mutex<Shard<A>>]>,
}

// SAFETY: `recv_waker` is initialized to a noop waker and only mutated by the receiver before
// senders are exposed. Senders only read it to wake the receiver. This makes shared references safe.
// The type is also safe to send between threads because all fields are `Send` under `A::Pointer:
// Send`; the waker cell is moved with `Shared`. Callers must not clone or expose senders before
// calling `Receiver::register`, ensuring waker mutation completes before senders can concurrently
// read it.
unsafe impl<A: intrusive_queue::Adapter> Sync for Shared<A> where A::Pointer: Send {}
unsafe impl<A: intrusive_queue::Adapter> Send for Shared<A> where A::Pointer: Send {}

impl<A: intrusive_queue::Adapter> Shared<A> {
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
        // SAFETY: The receiver initializes the waker before senders are exposed. Senders only read
        // the waker after that point.
        unsafe { (&*self.recv_waker.get()).wake_by_ref() };
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
    Sender<intrusive_queue::EntryAdapter<T>>,
    Receiver<intrusive_queue::EntryAdapter<T>>,
) {
    new_with_adapter::<intrusive_queue::EntryAdapter<T>>(shard_count)
}

/// Creates a sharded intrusive queue channel.
///
/// Call [`Receiver::register`] immediately after creation and before cloning or exposing the
/// returned sender to another thread.
pub fn new_with_adapter<A: intrusive_queue::Adapter>(
    shard_count: usize,
) -> (Sender<A>, Receiver<A>) {
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
                queue: intrusive_queue::List::new(),
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
        waker_registered: AtomicBool::new(false),
        recv_waker: UnsafeCell::new(s2n_quic_core::task::waker::noop()),
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

pub struct Sender<A: intrusive_queue::Adapter> {
    next_shard: usize,
    shared: Arc<Shared<A>>,
}

impl<A: intrusive_queue::Adapter> Clone for Sender<A> {
    fn clone(&self) -> Self {
        self.shared.sender_count.fetch_add(1, Ordering::Relaxed);
        Self {
            next_shard: self.shared.allocate_sender_shard(),
            shared: self.shared.clone(),
        }
    }
}

impl<A: intrusive_queue::Adapter> Drop for Sender<A> {
    fn drop(&mut self) {
        if self.shared.sender_count.fetch_sub(1, Ordering::Release) == 1 {
            self.shared.wake_receiver();
        }
    }
}

impl<A: intrusive_queue::Adapter> Sender<A> {
    #[inline(always)]
    fn next_shard(&mut self) -> usize {
        let shard = self.next_shard;
        // The creation-time stride spreads senders out; each sender then walks adjacent shards to
        // avoid repeatedly colliding with other senders using the same stride.
        self.next_shard = (shard + 1) & self.shared.shard_mask;
        shard
    }

    pub fn send_batch(
        &mut self,
        mut list: intrusive_queue::List<A>,
    ) -> Result<(), intrusive_queue::List<A>> {
        debug_assert!(
            self.shared.waker_registered.load(Ordering::Acquire),
            "receiver waker must be registered before exposing senders"
        );

        if list.is_empty() {
            return Ok(());
        }

        let shard = self.next_shard();
        let mut queue = lock(&self.shared.shards[shard]);

        if !queue.is_open {
            return Err(list);
        }

        let was_empty = queue.queue.is_empty();
        queue.queue.append(&mut list);
        drop(queue);

        if was_empty {
            self.shared.set_occupied(shard);
            self.shared.wake_receiver();
        }

        Ok(())
    }
}

impl<A: intrusive_queue::Adapter> super::super::UnboundedSender<intrusive_queue::List<A>>
    for Sender<A>
{
    #[inline(always)]
    fn send(&mut self, list: intrusive_queue::List<A>) -> Result<(), intrusive_queue::List<A>> {
        self.send_batch(list)
    }
}

impl<A: intrusive_queue::Adapter> super::super::Sender<intrusive_queue::List<A>> for Sender<A> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        slot: &mut core::mem::MaybeUninit<intrusive_queue::List<A>>,
    ) -> Poll<Result<(), ()>> {
        // SAFETY: the Sender trait requires callers to provide an initialized slot.
        let list = unsafe { slot.assume_init_read() };
        match self.send_batch(list) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(list) => {
                slot.write(list);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<A: intrusive_queue::Adapter> {
    next_shard: usize,
    local_occupancy: Box<[u64]>,
    shared: Arc<Shared<A>>,
}

impl<A: intrusive_queue::Adapter> Drop for Receiver<A> {
    fn drop(&mut self) {
        for shard in self.shared.shards.iter() {
            lock(shard).is_open = false;
        }
    }
}

impl<A: intrusive_queue::Adapter> Receiver<A> {
    /// Registers the receiver waker.
    ///
    /// This channel expects the receiver to register immediately after channel creation, before any
    /// sender is cloned or exposed to another thread.
    pub fn register(&self, waker: &Waker) {
        // SAFETY: callers must complete registration before cloning or exposing senders. After
        // that point, senders may concurrently read the waker.
        unsafe {
            let old_waker = core::mem::replace(&mut *self.shared.recv_waker.get(), waker.clone());
            drop(old_waker);
        }
        self.shared.waker_registered.store(true, Ordering::Release);
    }

    #[inline(always)]
    fn try_recv(&mut self) -> TryRecv<A> {
        // Only consume one occupied bit per receive attempt so stale occupancy bookkeeping stays
        // visible to debug builds instead of being hidden by looking for another ready shard.
        if let Some(shard) = self.next_occupied() {
            let mut queue = lock(&self.shared.shards[shard]);
            debug_assert!(
                !queue.queue.is_empty(),
                "occupancy bit set for an empty shard"
            );

            // In release builds, preserve the receive contract by returning a valid list, even if
            // it is empty, instead of continuing to scan for a non-empty shard. The debug assertion
            // above catches stale occupancy during testing.
            let list = core::mem::take(&mut queue.queue);
            return TryRecv::Ready(list);
        }

        TryRecv::Empty
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

impl<A: intrusive_queue::Adapter> super::super::Receiver<intrusive_queue::List<A>> for Receiver<A> {
    #[inline(always)]
    fn poll_recv(
        &mut self,
        // The receiver waker is registered explicitly before senders are exposed; the channel
        // trait still requires a context parameter here.
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<intrusive_queue::List<A>>> {
        if let TryRecv::Ready(list) = self.try_recv() {
            return Poll::Ready(Some(list));
        }

        if self.shared.sender_count.load(Ordering::Acquire) == 0 {
            if let TryRecv::Ready(list) = self.try_recv() {
                return Poll::Ready(Some(list));
            }

            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}

enum TryRecv<A: intrusive_queue::Adapter> {
    Ready(intrusive_queue::List<A>),
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        intrusive_queue::{Entry, Queue},
        socket::channel::{Receiver as _, UnboundedSender as _},
    };
    use core::task::Poll;

    fn noop_cx() -> core::task::Context<'static> {
        let waker = s2n_quic_core::task::waker::noop();
        let waker_ref = Box::leak(Box::new(waker));
        core::task::Context::from_waker(waker_ref)
    }

    fn register<A: intrusive_queue::Adapter>(rx: &mut Receiver<A>) {
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

    #[test]
    #[should_panic(expected = "shard count must be a power of two")]
    fn rejects_non_power_of_two_shards() {
        let _ = new::<u32>(3);
    }

    #[test]
    fn drains_entire_shard() {
        let (mut tx, mut rx) = new::<u32>(1);
        let mut cx = noop_cx();
        register(&mut rx);

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));

        tx.send(list([1, 2, 3])).unwrap();

        let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx) else {
            panic!("expected drained list");
        };
        assert_eq!(values(&list), vec![1, 2, 3]);

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));
    }

    #[test]
    fn sender_creation_selects_initial_shard() {
        let (mut tx0, mut rx) = new::<u32>(4);
        register(&mut rx);
        let mut tx1 = tx0.clone();
        let mut tx2 = tx0.clone();
        let mut tx3 = tx0.clone();
        let mut cx = noop_cx();

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));

        tx3.send(list([3])).unwrap();
        tx2.send(list([2])).unwrap();
        tx1.send(list([1])).unwrap();
        tx0.send(list([0])).unwrap();

        let mut received = vec![];
        for _ in 0..4 {
            let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx) else {
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
        register(&mut rx);

        for value in 0..4 {
            tx.send(list([value])).unwrap();
        }

        for expected in 0..4 {
            let Poll::Ready(Some(list)) = rx.poll_recv(&mut cx) else {
                panic!("expected drained list");
            };
            assert_eq!(values(&list), vec![expected]);
        }
    }

    #[test]
    fn sender_drop_closes_receiver() {
        let (tx, mut rx) = new::<u32>(2);
        let mut cx = noop_cx();

        assert!(matches!(rx.poll_recv(&mut cx), Poll::Pending));
        drop(tx);
        assert!(matches!(rx.poll_recv(&mut cx), Poll::Ready(None)));
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

        rx.local_occupancy[0] = 1;

        let _ = rx.poll_recv(&mut cx);
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

                    core::future::poll_fn(|cx| {
                        rx.register(cx.waker());
                        let (registered, cvar) = &*registered_rx;
                        *registered.lock().unwrap() = true;
                        cvar.notify_one();
                        Poll::Ready(())
                    })
                    .await;

                    loop {
                        let item = core::future::poll_fn(|cx| rx.poll_recv(cx)).await;

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
