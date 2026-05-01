// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Non-Send intrusive queue channel for busy-poll runtimes.
//!
//! The sender has no backpressure - it can always push entries to the queue.
//! The receiver drains the queue until empty, returning Pending when empty.

use crate::intrusive_queue;
use std::{cell::UnsafeCell, rc::Rc, task::Poll};

struct Shared<T> {
    queue: UnsafeCell<intrusive_queue::Queue<T>>,
}

impl<T> Shared<T> {
    #[inline(always)]
    fn is_alive(self: &Rc<Self>) -> bool {
        // Sender and receiver both hold a reference
        Rc::strong_count(self) == 2
    }
}

pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Rc::new(Shared {
        queue: UnsafeCell::new(intrusive_queue::Queue::new()),
    });
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}

pub struct Sender<T> {
    shared: Rc<Shared<T>>,
}

impl<T> super::super::UnboundedSender<intrusive_queue::Entry<T>> for Sender<T> {
    #[inline(always)]
    fn send(&mut self, value: intrusive_queue::Entry<T>) -> Result<(), intrusive_queue::Entry<T>> {
        if !self.shared.is_alive() {
            return Err(value);
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();
            queue.push_back(value);
        }

        Ok(())
    }
}

impl<T> super::super::Sender<intrusive_queue::Entry<T>> for Sender<T> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive_queue::Entry<T>>,
    ) -> Poll<Result<(), ()>> {
        if !self.shared.is_alive() {
            return Poll::Ready(Err(()));
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();
            let entry = value.assume_init_read();
            queue.push_back(entry);
        }

        Poll::Ready(Ok(()))
    }
}

impl<T> Sender<T> {
    /// Send a batch of entries by appending them to the queue.
    ///
    /// This is more efficient than sending entries one at a time.
    #[inline(always)]
    pub fn send_batch(
        &mut self,
        mut batch: intrusive_queue::Queue<T>,
    ) -> Result<(), intrusive_queue::Queue<T>> {
        if batch.is_empty() {
            return Ok(());
        }

        if !self.shared.is_alive() {
            return Err(batch);
        }

        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();
            queue.append(&mut batch);
        }

        Ok(())
    }
}

impl<T> super::super::UnboundedSender<intrusive_queue::Queue<T>> for Sender<T> {
    #[inline(always)]
    fn send(
        &mut self,
        batch: intrusive_queue::Queue<T>,
    ) -> Result<(), intrusive_queue::Queue<T>> {
        self.send_batch(batch)
    }
}

impl<T> super::super::Sender<intrusive_queue::Queue<T>> for Sender<T> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<intrusive_queue::Queue<T>>,
    ) -> Poll<Result<(), ()>> {
        let batch = unsafe { value.assume_init_read() };

        if batch.is_empty() {
            return Poll::Ready(Ok(()));
        }

        match self.send_batch(batch) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(returned_batch) => {
                // Put the batch back and signal closed
                value.write(returned_batch);
                Poll::Ready(Err(()))
            }
        }
    }
}

pub struct Receiver<T> {
    shared: Rc<Shared<T>>,
}

impl<T> super::super::Receiver<intrusive_queue::Entry<T>> for Receiver<T> {
    #[inline(always)]
    fn poll_recv(
        &mut self,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<intrusive_queue::Entry<T>>> {
        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();

            if let Some(entry) = queue.pop_front() {
                return Poll::Ready(Some(entry));
            }
        }

        if !self.shared.is_alive() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}

impl<T> super::super::Receiver<intrusive_queue::Queue<T>> for Receiver<T> {
    #[inline(always)]
    fn poll_recv(
        &mut self,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<intrusive_queue::Queue<T>>> {
        unsafe {
            // SAFETY: the Shared struct is non-Send and we have exclusive access through &mut
            let queue = &mut *self.shared.queue.get();

            if !queue.is_empty() {
                // Drain all available entries into a batch
                let batch = core::mem::take(queue);
                return Poll::Ready(Some(batch));
            }
        }

        if !self.shared.is_alive() {
            return Poll::Ready(None);
        }

        Poll::Pending
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}
