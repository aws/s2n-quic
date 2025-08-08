// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::ensure;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Closed;

#[derive(Clone, Copy, Debug)]
pub struct Capacity {
    /// Set the upper bound of items in the queue
    pub max: usize,
    /// Initial allocated capacity
    pub initial: usize,
}

impl From<usize> for Capacity {
    #[inline]
    fn from(capacity: usize) -> Self {
        Self {
            max: capacity,
            initial: capacity,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Priority {
    #[default]
    Required,
    Optional,
}

pub struct RingDeque<T, W = ()> {
    inner: Arc<Mutex<Inner<T, W>>>,
}

impl<T, W> Clone for RingDeque<T, W> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T, W: Default + RecvWaker> RingDeque<T, W> {
    #[inline]
    pub fn new<C: Into<Capacity>>(capacity: C) -> Self {
        let waker = W::default();
        Self::with_waker(capacity, waker)
    }
}

impl<T, W: RecvWaker> RingDeque<T, W> {
    #[inline]
    pub fn with_waker<C: Into<Capacity>>(capacity: C, recv_waker: W) -> Self {
        let capacity = capacity.into();
        let queue = VecDeque::with_capacity(capacity.initial);
        let inner = Inner {
            open: true,
            queue,
            capacity: capacity.max,
            recv_waker,
        };
        let inner = Arc::new(Mutex::new(inner));
        RingDeque { inner }
    }

    #[inline]
    pub fn push_back(&self, value: T) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;

        let prev = if inner.capacity == inner.queue.len() {
            inner.queue.pop_front()
        } else {
            None
        };

        inner.queue.push_back(value);
        let waker = inner.recv_waker.take();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(prev)
    }

    #[inline]
    pub fn push_front(&self, value: T) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;

        let prev = if inner.capacity == inner.queue.len() {
            inner.queue.pop_back()
        } else {
            None
        };

        inner.queue.push_front(value);
        let waker = inner.recv_waker.take();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(prev)
    }

    #[inline]
    pub fn poll_swap(
        &self,
        cx: &mut Context<'_>,
        out: &mut VecDeque<T>,
    ) -> Poll<Result<(), Closed>> {
        debug_assert!(out.is_empty());
        let mut inner = self.lock()?;
        if inner.queue.is_empty() {
            inner.recv_waker.update(cx);
            Poll::Pending
        } else {
            core::mem::swap(&mut inner.queue, out);
            Ok(()).into()
        }
    }

    #[inline]
    pub fn poll_pop_back(&self, cx: &mut Context<'_>) -> Poll<Result<T, Closed>> {
        let mut inner = self.lock()?;
        if let Some(item) = inner.queue.pop_back() {
            Ok(item).into()
        } else {
            inner.recv_waker.update(cx);
            Poll::Pending
        }
    }

    #[inline]
    pub fn pop_back(&self) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;
        Ok(inner.queue.pop_back())
    }

    #[inline]
    pub fn pop_back_if<F>(&self, priority: Priority, check: F) -> Result<Option<T>, Closed>
    where
        F: FnOnce(&T) -> bool,
    {
        let inner = match priority {
            Priority::Required => Some(self.lock()?),
            Priority::Optional => self.try_lock()?,
        };

        let Some(mut inner) = inner else {
            return Ok(None);
        };

        let Some(back) = inner.queue.back() else {
            return Ok(None);
        };

        if check(back) {
            Ok(inner.queue.pop_back())
        } else {
            Ok(None)
        }
    }

    #[inline]
    pub fn poll_pop_front(&self, cx: &mut Context<'_>) -> Poll<Result<T, Closed>> {
        let mut inner = self.lock()?;
        if let Some(item) = inner.queue.pop_front() {
            Ok(item).into()
        } else {
            inner.recv_waker.update(cx);
            Poll::Pending
        }
    }

    #[inline]
    pub fn pop_front(&self) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;
        Ok(inner.queue.pop_front())
    }

    #[inline]
    pub fn pop_front_if<F>(&self, priority: Priority, check: F) -> Result<Option<T>, Closed>
    where
        F: FnOnce(&T) -> bool,
    {
        let inner = match priority {
            Priority::Required => Some(self.lock()?),
            Priority::Optional => self.try_lock()?,
        };

        let Some(mut inner) = inner else {
            return Ok(None);
        };

        let Some(back) = inner.queue.front() else {
            return Ok(None);
        };

        if check(back) {
            Ok(inner.queue.pop_front())
        } else {
            Ok(None)
        }
    }

    #[inline]
    pub fn close(&self) -> Result<(), Closed> {
        let mut inner = self.lock()?;
        inner.open = false;
        let waker = inner.recv_waker.take();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(())
    }

    #[inline]
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner<T, W>>, Closed> {
        let inner = self.inner.lock().unwrap();
        ensure!(inner.open, Err(Closed));
        Ok(inner)
    }

    #[inline]
    fn try_lock(&self) -> Result<Option<std::sync::MutexGuard<'_, Inner<T, W>>>, Closed> {
        use std::sync::TryLockError;
        let inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err(Closed),
        };
        ensure!(inner.open, Err(Closed));
        Ok(Some(inner))
    }
}

struct Inner<T, W> {
    open: bool,
    queue: VecDeque<T>,
    capacity: usize,
    recv_waker: W,
}

/// An interface for storing a waker in the synchronized queue
///
/// This can be used for implementing single consumer queues without
/// additional machinery for storing wakers.
pub trait RecvWaker {
    /// Takes the current waker and returns it, if set
    ///
    /// This is to avoid calling `wake` while holding the lock on the queue
    /// to avoid contention.
    fn take(&mut self) -> Option<Waker>;
    fn update(&mut self, cx: &mut core::task::Context<'_>);
}

impl RecvWaker for () {
    #[inline(always)]
    fn take(&mut self) -> Option<Waker> {
        None
    }

    #[inline(always)]
    fn update(&mut self, _cx: &mut core::task::Context<'_>) {
        panic!("polling is disabled");
    }
}

impl RecvWaker for Option<Waker> {
    #[inline(always)]
    fn take(&mut self) -> Option<Waker> {
        self.take()
    }

    #[inline(always)]
    fn update(&mut self, cx: &mut core::task::Context<'_>) {
        let new_waker = cx.waker();
        match self {
            Some(waker) => {
                if !waker.will_wake(new_waker) {
                    *self = Some(new_waker.clone());
                }
            }
            None => *self = Some(new_waker.clone()),
        }
    }
}
