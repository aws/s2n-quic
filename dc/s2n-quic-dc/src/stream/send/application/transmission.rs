// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::send::state::transmission;
use crossbeam_queue::{ArrayQueue, SegQueue};
use s2n_quic_core::{ensure, varint::VarInt};
use std::collections::VecDeque;

pub use transmission::Info;

#[derive(Debug)]
pub struct Event<Buffer> {
    pub packet_number: VarInt,
    pub info: Info<Buffer>,
    pub has_more_app_data: bool,
}

pub struct Queue<Buffer> {
    queue: SegQueue<Vec<Event<Buffer>>>,
    free_batches: ArrayQueue<Vec<Event<Buffer>>>,
}

impl<Buffer> Default for Queue<Buffer> {
    #[inline]
    fn default() -> Self {
        Self {
            queue: SegQueue::new(),
            free_batches: ArrayQueue::new(32),
        }
    }
}

impl<Buffer> Queue<Buffer> {
    #[inline]
    pub fn alloc_batch(&self, batch_size: usize) -> Vec<Event<Buffer>> {
        self.free_batches
            .pop()
            .filter(|batch| batch.capacity() >= batch_size)
            .unwrap_or_else(|| Vec::with_capacity(batch_size))
    }

    #[inline]
    pub fn push_batch(&self, batch: Vec<Event<Buffer>>) {
        ensure!(!batch.is_empty());
        self.queue.push(batch);
    }

    #[inline]
    pub fn drain(&self) -> impl Iterator<Item = Event<Buffer>> + '_ {
        Drain {
            queue: self,
            current: VecDeque::new(),
        }
    }
}

pub struct Drain<'a, Buffer> {
    current: VecDeque<Event<Buffer>>,
    queue: &'a Queue<Buffer>,
}

impl<'a, Buffer> Iterator for Drain<'a, Buffer> {
    type Item = Event<Buffer>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(event) = self.current.pop_front() {
                return Some(event);
            }

            if let Some(events) = self.queue.queue.pop() {
                // https://doc.rust-lang.org/std/collections/struct.VecDeque.html#impl-From%3CVec%3CT,+A%3E%3E-for-VecDeque%3CT,+A%3E
                // > This conversion is guaranteed to run in O(1) time and to not re-allocate the Vec’s
                // > buffer or allocate any additional memory.
                let prev = core::mem::replace(&mut self.current, events.into());
                // https://doc.rust-lang.org/std/collections/struct.VecDeque.html#impl-From%3CVecDeque%3CT,+A%3E%3E-for-Vec%3CT,+A%3E
                // > This never needs to re-allocate, but does need to do O(n) data movement if the circular buffer
                // > doesn’t happen to be at the beginning of the allocation.
                //
                // NOTE prev should be empty at this point so this conversion is free
                debug_assert!(prev.is_empty());
                let _ = self.queue.free_batches.push(prev.into());
            } else {
                return None;
            }
        }
    }
}
