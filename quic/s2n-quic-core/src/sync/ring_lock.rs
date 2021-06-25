// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::{
    atomic::{AtomicUsize, Ordering},
    atomic_waker::AtomicWaker,
    Arc,
};
use core::task::{Context, Poll};

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub index: usize,
    pub len: usize,
}

#[derive(Debug)]
struct Ring {
    capacity: usize,
    watermark: usize,

    producer_waker: AtomicWaker,
    producer_head: AtomicUsize,

    consumer_waker: AtomicWaker,
    consumer_head: AtomicUsize,
}

impl Ring {
    pub fn new(capacity: usize, watermark: usize) -> Self {
        Self {
            capacity,
            watermark,
            producer_waker: AtomicWaker::new(),
            producer_head: AtomicUsize::new(0),
            consumer_waker: AtomicWaker::new(),
            consumer_head: AtomicUsize::new(0),
        }
    }

    pub fn producer_region(&self) -> Region {
        let producer_head = self.producer_head.load(Ordering::Relaxed);
        let consumer_head = self.consumer_head.load(Ordering::SeqCst);
        let (producer, _) = self.regions(producer_head, consumer_head);
        producer
    }

    pub fn produce(&self, len: usize) {
        let producer_head = self
            .producer_head
            .fetch_add(len, Ordering::SeqCst)
            .wrapping_add(len);
        let consumer_head = self.consumer_head.load(Ordering::SeqCst);
        let (_, consumer) = self.regions(producer_head, consumer_head);

        if consumer.len >= self.watermark {
            self.consumer_waker.wake();
        }
    }

    pub fn consumer_region(&self) -> Region {
        let producer_head = self.producer_head.load(Ordering::SeqCst);
        let consumer_head = self.consumer_head.load(Ordering::Relaxed);
        let (_, consumer) = self.regions(producer_head, consumer_head);
        consumer
    }

    #[inline(always)]
    fn regions(&self, producer_head: usize, consumer_head: usize) -> (Region, Region) {
        let capacity = self.capacity;

        let consumer_len = if producer_head >= consumer_head {
            producer_head - consumer_head
        } else {
            usize::MAX - consumer_head + producer_head
        };

        debug_assert!(consumer_len <= capacity);

        let producer_len = capacity - consumer_len;

        let consumer_index = consumer_head % capacity;
        let producer_index = producer_head % capacity;

        (
            Region {
                index: producer_index,
                len: producer_len,
            },
            Region {
                index: consumer_index,
                len: consumer_len,
            },
        )
    }

    pub fn consume(&self, len: usize) {
        let producer_head = self.producer_head.load(Ordering::SeqCst);
        let consumer_head = self
            .consumer_head
            .fetch_add(len, Ordering::SeqCst)
            .wrapping_add(len);
        let (producer, _) = self.regions(producer_head, consumer_head);

        if producer.len >= self.watermark {
            self.producer_waker.wake();
        }
    }
}

#[derive(Debug)]
pub struct Producer {
    ring: Arc<Ring>,
    acquired: usize,
}

impl Producer {
    pub fn poll_region(&mut self, cx: &mut Context<'_>) -> Poll<Region> {
        // first we check to see if we have any capacity before registering
        let region = self.ring.producer_region();

        if region.len > 0 {
            self.acquired = region.len;
            return Poll::Ready(region);
        }

        self.ring.producer_waker.register(cx.waker());

        // check region after register to avoid lost notifications
        let region = self.ring.producer_region();
        if region.len > 0 {
            self.acquired = region.len;
            return Poll::Ready(region);
        }

        Poll::Pending
    }

    pub fn current_region(&mut self) -> Region {
        let region = self.ring.producer_region();
        self.acquired = region.len;
        region
    }

    pub fn push(&mut self, len: usize) {
        assert!(self.acquired >= len);
        unsafe { self.push_unchecked(len) }
    }

    /// # Safety
    ///
    /// Callers must ensure the pushed len does not exceed the acquired region len
    pub unsafe fn push_unchecked(&mut self, len: usize) {
        self.acquired -= len;
        self.ring.produce(len);
    }
}

#[derive(Debug)]
pub struct Consumer {
    ring: Arc<Ring>,
    acquired: usize,
}

impl Consumer {
    pub fn poll_region(&mut self, cx: &mut Context<'_>) -> Poll<Region> {
        // first we check to see if we have any capacity before registering
        let region = self.ring.consumer_region();

        if region.len > 0 {
            self.acquired = region.len;
            return Poll::Ready(region);
        }

        self.ring.consumer_waker.register(cx.waker());

        // check region after register to avoid lost notifications
        let region = self.ring.consumer_region();
        if region.len > 0 {
            self.acquired = region.len;
            return Poll::Ready(region);
        }

        Poll::Pending
    }

    pub fn pop(&mut self, len: usize) {
        assert!(self.acquired >= len);
        unsafe { self.pop_unchecked(len) }
    }

    /// # Safety
    ///
    /// Callers must ensure the popped len does not exceed the acquired region len
    pub unsafe fn pop_unchecked(&mut self, len: usize) {
        self.acquired -= len;
        self.ring.consume(len);
    }
}

#[derive(Debug)]
pub struct Builder {
    capacity: usize,
    watermark: usize,
}

impl Builder {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 2);
        // TODO assert capacity is 2^n
        Self {
            capacity,
            watermark: 1,
        }
    }

    pub fn watermark(&mut self, watermark: usize) -> &mut Self {
        self.watermark = watermark.max(1);
        self
    }

    pub fn build(self) -> (Producer, Consumer) {
        let ring = Ring::new(self.capacity, self.watermark);
        let ring = Arc::new(ring);
        let producer = Producer {
            ring: ring.clone(),
            acquired: 0,
        };
        let consumer = Consumer { ring, acquired: 0 };
        (producer, consumer)
    }
}

pub fn new(capacity: usize) -> (Producer, Consumer) {
    Builder::new(capacity).build()
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use bolero::check;

    #[test]
    fn ring_test() {
        check!().with_type::<(usize, Vec<u8>, Vec<u8>)>().for_each(
            |(start_index, producer_instr, consumer_instr)| {
                struct Model {
                    ring: Ring,
                    producer_cap: usize,
                    consumer_cap: usize,
                }

                impl Model {
                    fn new(capacity: usize, start_index: usize) -> Self {
                        let ring = Ring::new(capacity, 1);
                        ring.producer_head.store(start_index, Ordering::SeqCst);
                        ring.consumer_head.store(start_index, Ordering::SeqCst);
                        Self {
                            ring,
                            producer_cap: capacity,
                            consumer_cap: 0,
                        }
                    }

                    fn produce(&mut self, len: usize) {
                        let produce = self.producer_cap.min(len);
                        self.ring.produce(produce);
                        self.producer_cap -= produce;
                        self.consumer_cap += produce;

                        self.check();
                    }

                    fn consume(&mut self, len: usize) {
                        let consume = self.consumer_cap.min(len);
                        self.ring.consume(consume);
                        self.consumer_cap -= consume;
                        self.producer_cap += consume;

                        self.check();
                    }

                    fn check(&self) {
                        let p = self.ring.producer_region();
                        assert_eq!(p.len, self.producer_cap, "{:#?}", self.ring);

                        let c = self.ring.consumer_region();
                        assert_eq!(c.len, self.consumer_cap, "{:#?}", self.ring);

                        assert_eq!(p.len + c.len, self.ring.capacity);
                    }
                }

                let mut model = Model::new(32, *start_index);

                for (produce, consume) in producer_instr.iter().zip(consumer_instr.iter()) {
                    model.produce(*produce as usize);
                    model.consume(*consume as usize);
                }
            },
        );
    }
}

/*
#[cfg(all(test, loom))]
mod loom_tests {
    use super::*;
    use bolero::{check, generator::*};
    use loom::thread;

    // TODO
    #[test]
    fn concurrency_test() {
        check!()
            .with_type::<(Vec<u8>, Vec<u8>)>()
            .for_each(|(producer_instr, consumer_instr)| {
                loom::model(move || {
                    let (producer, consumer) = new(64);

                    thread::spawn(move || for instr in producer_instr {});
                })
            })
    }
}
*/
