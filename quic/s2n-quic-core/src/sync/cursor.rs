// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    marker::PhantomData,
    num::Wrapping,
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};

pub struct Builder<T: Copy> {
    pub producer: NonNull<AtomicU32>,
    pub consumer: NonNull<AtomicU32>,
    pub data: NonNull<T>,
    pub size: u32,
}

impl<T: Copy> Builder<T> {
    /// Builds a cursor for a producer
    ///
    /// # Safety
    ///
    /// * This should only be called for the producer
    /// * The pointers should outlive the `Cursor`
    #[inline]
    pub unsafe fn build_producer(self) -> Cursor<T> {
        let mut cursor = self.build();
        cursor.init_producer();
        cursor
    }

    /// Builds a cursor for a consumer
    ///
    /// # Safety
    ///
    /// * This should only be called for the consumer
    /// * The pointers should outlive the `Cursor`
    #[inline]
    pub unsafe fn build_consumer(self) -> Cursor<T> {
        self.build()
    }

    #[inline]
    const fn build(self) -> Cursor<T> {
        let Self {
            producer,
            consumer,
            data,
            size,
        } = self;

        debug_assert!(size.is_power_of_two());

        let mask = size - 1;

        Cursor {
            cached_consumer: Wrapping(0),
            cached_producer: Wrapping(0),
            cached_len: 0,
            size,
            mask,
            producer,
            consumer,
            data,
            entry: PhantomData,
        }
    }
}

/// A structure for tracking a ring shared between a producer and consumer
///
/// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L34-L42).
#[derive(Debug)]
pub struct Cursor<T: Copy> {
    /// A cached value for the producer cursor index
    ///
    /// This is stored locally to avoid atomic synchronization, if possible
    cached_producer: Wrapping<u32>,
    /// A cached value for the consumer cursor index
    ///
    /// This is stored locally to avoid atomic synchronization, if possible
    cached_consumer: Wrapping<u32>,
    /// A mask value to ensure validity of cursor indexes
    ///
    /// This value assumes that the size of the ring is a power of two
    mask: u32,
    /// The number of entries in the ring
    ///
    /// This value MUST be a power of two
    size: u32,
    /// Points to the producer cursor index
    producer: NonNull<AtomicU32>,
    /// Points to the consumer cursor index
    consumer: NonNull<AtomicU32>,
    /// Points to the values in the ring
    data: NonNull<T>,
    /// A cached value of the computed number of entries for the owner of the `Cursor`
    ///
    /// Since the `acquire` paths are critical to efficiency, we store a derived length to avoid
    /// performing the math over and over again. As such this value needs to be kept in sync with
    /// the `cached_consumer` and `cached_producer`.
    cached_len: u32,
    /// Holds the type of the entries in the ring
    entry: PhantomData<T>,
}

impl<T: Copy> Cursor<T> {
    /// Initializes a producer cursor
    ///
    /// # Safety
    ///
    /// This should only be called by a producer
    #[inline]
    unsafe fn init_producer(&mut self) {
        // increment the consumer cursor by the total size to avoid doing an addition inside
        // `cached_producer`
        //
        // See
        // https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L99-L104
        self.cached_consumer += self.size;
        self.cached_len = self.cached_producer_len();

        debug_assert!(self.cached_len <= self.size);
    }

    /// Returns a reference to the producer atomic cursor
    #[inline]
    pub fn producer(&self) -> &AtomicU32 {
        unsafe { &*self.producer.as_ptr() }
    }

    /// Returns a reference to the producer atomic cursor
    #[inline]
    pub fn consumer(&self) -> &AtomicU32 {
        unsafe { &*self.consumer.as_ptr() }
    }

    /// Returns the overall size of the ring
    pub const fn capacity(&self) -> u32 {
        self.size
    }

    /// Acquires a cursor index for a producer half
    ///
    /// The `watermark` can be provided to avoid synchronization by reusing the cached cursor
    /// value.
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L92).
    #[inline]
    pub fn acquire_producer(&mut self, watermark: u32) -> u32 {
        // cap the watermark by the max size of the ring to prevent needless loads
        let watermark = watermark.min(self.size);
        let free = self.cached_len;

        // if we have enough space, then return the cached value
        if free >= watermark {
            return free;
        }

        let mut new_value = self.consumer().load(Ordering::Acquire);

        // Our cached copy has the size added so we also need to add the size here when comparing
        //
        // See `Self::init_producer` for more details
        new_value = new_value.wrapping_add(self.size);

        if self.cached_consumer.0 == new_value {
            return free;
        }

        self.cached_consumer.0 = new_value;

        self.cached_len = self.cached_producer_len();

        debug_assert!(self.cached_len <= self.size);

        self.cached_len
    }

    /// Returns the cached producer cursor which is also maxed by the cursor mask
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L60).
    #[inline]
    pub fn cached_producer(&self) -> u32 {
        // Wrap the cursor around the size of the ring
        //
        // Masking with a `2^N - 1` value is the same as a mod operation, just more efficient
        self.cached_producer.0 & self.mask
    }

    /// Returns the cached number of available entries for the consumer
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L94).
    #[inline]
    pub fn cached_producer_len(&self) -> u32 {
        (self.cached_consumer - self.cached_producer).0
    }

    /// Releases a `len` number of entries from the producer to the consumer.
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L135).
    ///
    /// The provided `len` should not exceed the number from `acquire_producer`. With
    /// debug_assertions enabled, this will panic if it occurs.
    #[inline]
    pub fn release_producer(&mut self, len: u32) {
        if cfg!(debug_assertions) {
            let max_len = self.cached_producer_len();
            assert!(max_len >= len, "available: {max_len}, requested: {len}");
        }
        self.cached_producer += len;
        self.cached_len -= len;

        debug_assert!(self.cached_len <= self.size);

        self.producer().fetch_add(len, Ordering::Release);
    }

    /// Acquires a cursor index for a consumer half
    ///
    /// The `watermark` can be provided to avoid synchronization by reusing the cached cursor
    /// value.
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L112).
    #[inline]
    pub fn acquire_consumer(&mut self, watermark: u32) -> u32 {
        // cap the watermark by the max size of the ring to prevent needless loads
        let watermark = watermark.min(self.size);
        let filled = self.cached_len;

        if filled >= watermark {
            return filled;
        }

        let new_value = self.producer().load(Ordering::Acquire);

        if self.cached_producer.0 == new_value {
            return filled;
        }

        self.cached_producer.0 = new_value;

        self.cached_len = self.cached_consumer_len();

        debug_assert!(self.cached_len <= self.size);

        self.cached_len
    }

    /// Returns the cached consumer cursor which is also maxed by the cursor mask
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L68).
    #[inline]
    pub fn cached_consumer(&self) -> u32 {
        // Wrap the cursor around the size of the ring
        //
        // Masking with a `2^N - 1` value is the same as a mod operation, just more efficient
        self.cached_consumer.0 & self.mask
    }

    /// Returns the cached number of available entries for the consumer
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L114).
    #[inline]
    pub fn cached_consumer_len(&self) -> u32 {
        (self.cached_producer - self.cached_consumer).0
    }

    /// Releases a `len` number of entries from the consumer to the producer.
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L160).
    ///
    /// The provided `len` should not exceed the number from `acquire_consumer`. With
    /// debug_assertions enabled, this will panic if it occurs.
    #[inline]
    pub fn release_consumer(&mut self, len: u32) {
        if cfg!(debug_assertions) {
            let max_len = self.cached_consumer_len();
            assert!(max_len >= len, "available: {max_len}, requested: {len}");
        }
        self.cached_consumer += len;
        self.cached_len -= len;

        debug_assert!(self.cached_len <= self.size);

        self.consumer().fetch_add(len, Ordering::Release);
    }

    /// Returns the current consumer entries
    ///
    /// # Safety
    ///
    /// This function MUST only be used by the consumer side.
    #[inline]
    pub unsafe fn consumer_data(&mut self) -> (&mut [T], &mut [T]) {
        let idx = self.cached_consumer();
        let len = self.cached_len;

        debug_assert_eq!(len, self.cached_consumer_len());

        self.mut_slices(idx as _, len as _)
    }

    /// Returns the current producer entries
    ///
    /// # Safety
    ///
    /// This function MUST only be used by the producer side.
    #[inline]
    pub unsafe fn producer_data(&mut self) -> (&mut [T], &mut [T]) {
        let idx = self.cached_producer();
        let len = self.cached_len;

        debug_assert_eq!(len, self.cached_producer_len());

        self.mut_slices(idx as _, len as _)
    }

    #[inline]
    pub const fn data_ptr(&self) -> NonNull<T> {
        self.data
    }

    /// Creates a pair of slices for a given cursor index and len
    #[inline]
    fn mut_slices(&mut self, idx: u64, len: u64) -> (&mut [T], &mut [T]) {
        if len == 0 {
            return (&mut [][..], &mut [][..]);
        }

        let ptr = self.data.as_ptr();

        if let Some(tail_len) = (idx + len).checked_sub(self.size as _) {
            let head_len = self.size as u64 - idx;
            debug_assert_eq!(head_len + tail_len, len);
            let head = unsafe { core::slice::from_raw_parts_mut(ptr.add(idx as _), head_len as _) };
            let tail = unsafe { core::slice::from_raw_parts_mut(ptr, tail_len as _) };
            (head, tail)
        } else {
            let slice = unsafe { core::slice::from_raw_parts_mut(ptr.add(idx as _), len as _) };
            (slice, &mut [][..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, generator::*};
    use core::cell::UnsafeCell;

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Op {
        ConsumerAcquire(u16),
        ConsumerRelease(u16),
        ProducerAcquire(u16),
        ProducerRelease(u16),
    }

    /// Implements a FIFO queue with a monotonic value
    #[derive(Clone, Debug, Default)]
    struct Oracle {
        size: u32,
        producer: u32,
        producer_value: u32,
        consumer: u32,
        consumer_value: u32,
    }

    impl Oracle {
        fn acquire_consumer(&mut self, actual: u32) {
            self.consumer = actual;
            self.invariants();
        }

        fn release_consumer(&mut self, count: u16) -> u32 {
            let count = self.consumer.min(count as u32);

            self.consumer -= count;
            self.consumer_value += count;

            self.invariants();
            count
        }

        fn validate_consumer(&self, (a, b): (&mut [u32], &mut [u32])) {
            for (actual, expected) in a.iter().chain(b.iter()).zip(self.consumer_value..) {
                assert_eq!(
                    expected, *actual,
                    "entry values should match {a:?} {b:?} {self:?}"
                );
            }
        }

        fn acquire_producer(&mut self, actual: u32) {
            self.producer = actual;
            self.invariants();
        }

        fn release_producer(&mut self, count: u16) -> u32 {
            let count = self.producer.min(count as u32);

            self.producer -= count;
            self.producer_value += count;

            self.invariants();
            count
        }

        fn fill_producer(&self, (a, b): (&mut [u32], &mut [u32])) {
            for (entry, value) in a.iter_mut().chain(b).zip(self.producer_value..) {
                *entry = value;
            }
        }

        fn invariants(&self) {
            assert!(
                self.size >= self.producer + self.consumer,
                "The producer and consumer indexes should always be less than the size"
            );
        }
    }

    fn stack_cursors<T, F, R>(init_cursor: u32, desc: &mut [T], exec: F) -> R
    where
        T: Copy,
        F: FnOnce(&mut Cursor<T>, &mut Cursor<T>) -> R,
    {
        let size = desc.len() as u32;
        debug_assert!(size.is_power_of_two());
        let producer_v = UnsafeCell::new(AtomicU32::new(init_cursor));
        let consumer_v = UnsafeCell::new(AtomicU32::new(init_cursor));
        let desc = UnsafeCell::new(desc);

        let producer_v = producer_v.get();
        let consumer_v = consumer_v.get();
        let desc = unsafe { (*desc.get()).as_mut_ptr() as *mut _ };

        let cached_consumer = Wrapping(init_cursor);
        let cached_producer = Wrapping(init_cursor);

        let mut producer: Cursor<T> = unsafe {
            Builder {
                size,
                producer: NonNull::new(producer_v).unwrap(),
                consumer: NonNull::new(consumer_v).unwrap(),
                data: NonNull::new(desc).unwrap(),
            }
            .build_producer()
        };

        producer.cached_consumer = cached_consumer;
        // the producer increments the consumer by `size` to optimize the math so we need to do the
        // same here
        producer.cached_consumer += size;
        producer.cached_producer = cached_producer;
        producer.cached_len = size;

        assert_eq!(producer.acquire_producer(u32::MAX), size);
        assert_eq!(producer.cached_len, producer.cached_producer_len());

        let mut consumer: Cursor<T> = unsafe {
            Builder {
                size,
                producer: NonNull::new(producer_v).unwrap(),
                consumer: NonNull::new(consumer_v).unwrap(),
                data: NonNull::new(desc).unwrap(),
            }
            .build_consumer()
        };

        consumer.cached_consumer = cached_consumer;
        consumer.cached_producer = cached_producer;
        consumer.cached_len = 0;

        assert_eq!(consumer.acquire_consumer(u32::MAX), 0);
        assert_eq!(consumer.cached_len, consumer.cached_consumer_len());

        exec(&mut producer, &mut consumer)
    }

    fn model(power_of_two: u8, init_cursor: u32, ops: &[Op]) {
        let size = (1 << power_of_two) as u32;

        #[cfg(not(kani))]
        let mut desc = vec![u32::MAX; size as usize];

        #[cfg(kani)]
        let mut desc = &mut [u32::MAX; (1 << MAX_POWER_OF_TWO) as usize][..size as usize];

        stack_cursors(init_cursor, &mut desc, |producer, consumer| {
            let mut oracle = Oracle {
                size,
                producer: size,
                ..Default::default()
            };

            for op in ops.iter().copied() {
                oracle.fill_producer(unsafe { producer.producer_data() });

                match op {
                    Op::ConsumerAcquire(count) => {
                        let actual = consumer.acquire_consumer(count as _);
                        oracle.acquire_consumer(actual);
                    }
                    Op::ConsumerRelease(count) => {
                        let oracle_count = oracle.release_consumer(count);
                        consumer.release_consumer(oracle_count);
                    }
                    Op::ProducerAcquire(count) => {
                        let actual = producer.acquire_producer(count as _);
                        oracle.acquire_producer(actual);
                    }
                    Op::ProducerRelease(count) => {
                        let oracle_count = oracle.release_producer(count);
                        producer.release_producer(oracle_count);
                    }
                }

                oracle.validate_consumer(unsafe { consumer.consumer_data() });
            }

            // final assertions
            let actual = consumer.acquire_consumer(u32::MAX);
            oracle.acquire_consumer(actual);
            let data = unsafe { consumer.consumer_data() };
            oracle.validate_consumer(data);
        });
    }

    #[cfg(not(kani))]
    type Ops = Vec<Op>;
    #[cfg(kani)]
    type Ops = crate::testing::InlineVec<Op, 4>;

    const MAX_POWER_OF_TWO: u8 = if cfg!(kani) { 2 } else { 10 };

    #[test]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri to run
    #[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
    fn oracle_test() {
        check!()
            .with_generator((1..=MAX_POWER_OF_TWO, produce(), produce::<Ops>()))
            .for_each(|(power_of_two, init_cursor, ops)| model(*power_of_two, *init_cursor, ops));
    }
}
