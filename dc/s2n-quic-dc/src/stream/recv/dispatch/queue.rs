// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::ring_deque::{Capacity, Closed, RecvWaker};
use core::task::{Context, Poll};
use s2n_quic_core::ensure;
use std::{collections::VecDeque, sync::Mutex, task::Waker};
use tracing::trace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The queue ID is not associated with a stream
    Unallocated,
    /// The queue has been closed and won't reopen
    Closed,
}

impl From<Closed> for Error {
    #[inline]
    fn from(_: Closed) -> Self {
        Self::Closed
    }
}

struct Inner<T> {
    queue: VecDeque<T>,
    capacity: usize,
    is_open: bool,
    has_receiver: bool,
    waker: Option<Waker>,
}

pub struct Queue<T> {
    inner: Mutex<Inner<T>>,
}

impl<T> Queue<T> {
    #[inline]
    pub fn new(capacity: Capacity) -> Self {
        Self {
            inner: Mutex::new(Inner {
                queue: VecDeque::with_capacity(capacity.initial),
                capacity: capacity.max,
                is_open: true,
                has_receiver: false,
                waker: None,
            }),
        }
    }

    #[inline]
    pub fn push(&self, value: T) -> Result<Option<T>, Error> {
        let mut inner = self.lock()?;
        // check if the queue is permanently closed
        ensure!(inner.is_open, Err(Error::Closed));
        // check if the queue is temporarily closed
        ensure!(inner.has_receiver, Err(Error::Unallocated));

        let prev = if inner.capacity == inner.queue.len() {
            inner.queue.pop_front()
        } else {
            None
        };

        trace!(has_overflow = prev.is_some(), "push");

        inner.queue.push_back(value);
        let waker = inner.waker.take();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(prev)
    }

    /// Bypasses closed checks and pushes items into the queue
    #[inline]
    pub fn force_push(&self, value: T) -> Option<T> {
        let Ok(mut inner) = self.lock() else {
            return Some(value);
        };

        let prev = if inner.capacity == inner.queue.len() {
            inner.queue.pop_front()
        } else {
            None
        };

        trace!(has_overflow = prev.is_some(), "push");

        inner.queue.push_back(value);
        let waker = inner.waker.take();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }

        prev
    }

    #[inline]
    pub fn pop(&self) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;
        trace!(has_items = !inner.queue.is_empty(), "pop");
        if let Some(item) = inner.queue.pop_front() {
            Ok(Some(item))
        } else {
            ensure!(inner.is_open, Err(Closed));
            Ok(None)
        }
    }

    #[inline]
    pub fn poll_pop(&self, cx: &mut Context) -> Poll<Result<T, Closed>> {
        let mut inner = self.lock()?;
        trace!(has_items = !inner.queue.is_empty(), "poll_pop");
        if let Some(item) = inner.queue.pop_front() {
            Ok(item).into()
        } else {
            ensure!(inner.is_open, Err(Closed).into());
            inner.waker.update(cx);
            Poll::Pending
        }
    }

    #[inline]
    pub fn poll_swap(&self, cx: &mut Context, items: &mut VecDeque<T>) -> Poll<Result<(), Closed>> {
        let mut inner = self.lock()?;
        trace!(items = 0, "poll_swap");
        if inner.queue.is_empty() {
            ensure!(inner.is_open, Err(Closed).into());
            inner.waker.update(cx);
            return Poll::Pending;
        }
        core::mem::swap(items, &mut inner.queue);
        Ok(()).into()
    }

    #[inline]
    pub fn has_receiver(&self) -> bool {
        self.lock().map(|inner| inner.has_receiver).unwrap_or(false)
    }

    #[inline]
    pub fn open_receiver(&self) {
        let Ok(mut inner) = self.lock() else {
            return;
        };
        trace!("opening receiver");
        inner.has_receiver = true;
    }

    #[inline]
    pub fn close_receiver(&self) {
        let Ok(mut inner) = self.lock() else {
            return;
        };
        trace!("closing receiver");
        inner.has_receiver = false;
        inner.waker = None;
        inner.queue.clear();
    }

    #[inline]
    pub fn close(&self) {
        let Ok(mut inner) = self.lock() else {
            return;
        };
        trace!("close queue");
        inner.is_open = false;
        // Leave the remaining items in the queue in case the receiver wants them.

        // Notify the receiver that the queue is now closed
        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }

    #[inline]
    fn lock(&self) -> Result<std::sync::MutexGuard<Inner<T>>, Closed> {
        self.inner.lock().map_err(|_| Closed)
    }
}
