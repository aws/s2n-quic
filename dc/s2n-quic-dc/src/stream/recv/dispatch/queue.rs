// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::Actor,
    sync::ring_deque::{Capacity, Closed, RecvWaker},
};
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_core::ensure;
use std::{collections::VecDeque, ops::ControlFlow, sync::Mutex, task::Waker};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The queue ID is not associated with a stream
    Unallocated,
    /// The queue has been closed and won't reopen
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    Stream,
    Control,
}

impl s2n_quic_core::probe::Arg for Half {
    #[inline]
    fn into_usdt(self) -> isize {
        match self {
            Half::Stream => 0,
            Half::Control => 1,
        }
    }
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
    app_waker: Option<Waker>,
    worker_waker: Option<Waker>,
}

impl<T> Inner<T> {
    fn take_wakers(&mut self) -> Wakers {
        Wakers {
            app_waker: self.app_waker.take(),
            worker_waker: self.worker_waker.take(),
        }
    }
}

impl<T> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("queue_len", &self.queue.len())
            .field("capacity", &self.capacity)
            .field("is_open", &self.is_open)
            .field("has_receiver", &self.has_receiver)
            .field("app_waker", &self.app_waker.is_some())
            .field("worker_waker", &self.worker_waker.is_some())
            .finish()
    }
}

pub struct Queue<T> {
    inner: Mutex<Inner<T>>,
    #[cfg(debug_assertions)]
    half: Half,
}

impl<T> Queue<T> {
    #[inline]
    pub fn new(capacity: Capacity, half: Half) -> Self {
        let _ = half;
        Self {
            inner: Mutex::new(Inner {
                queue: VecDeque::with_capacity(capacity.initial),
                capacity: capacity.max,
                is_open: true,
                has_receiver: false,
                app_waker: None,
                worker_waker: None,
            }),
            #[cfg(debug_assertions)]
            half,
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

        inner.queue.push_back(value);
        let wakers = inner.take_wakers();
        drop(inner);
        drop(wakers);

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

        inner.queue.push_back(value);
        let wakers = inner.take_wakers();
        drop(inner);
        drop(wakers);

        prev
    }

    #[inline]
    pub fn pop(&self) -> Result<Option<T>, Closed> {
        let mut inner = self.lock()?;
        if let Some(item) = inner.queue.pop_front() {
            return Ok(Some(item));
        }

        ensure!(inner.is_open, Err(Closed));
        Ok(None)
    }

    #[inline]
    pub fn poll_pop(&self, cx: &mut Context, actor: Actor) -> Poll<Result<T, Closed>> {
        let mut inner = self.lock()?;
        if let Some(item) = inner.queue.pop_front() {
            return Ok(item).into();
        }

        ensure!(inner.is_open, Err(Closed).into());

        match actor {
            Actor::Application => &mut inner.app_waker,
            Actor::Worker => &mut inner.worker_waker,
        }
        .update(cx);

        Poll::Pending
    }

    #[inline]
    pub fn poll_swap(
        &self,
        cx: &mut Context,
        actor: Actor,
        items: &mut VecDeque<T>,
    ) -> Poll<Result<(), Closed>> {
        debug_assert!(items.is_empty(), "destination items should be empty");

        let mut inner = self.lock()?;
        if inner.queue.is_empty() {
            ensure!(inner.is_open, Err(Closed).into());

            match actor {
                Actor::Application => &mut inner.app_waker,
                Actor::Worker => &mut inner.worker_waker,
            }
            .update(cx);

            return Poll::Pending;
        }

        core::mem::swap(items, &mut inner.queue);
        Ok(()).into()
    }

    #[inline]
    pub fn open_receivers(&self, control: &Self) -> Result<(), Closed> {
        #[cfg(debug_assertions)]
        {
            assert_eq!(self.half, Half::Stream);
            assert_eq!(control.half, Half::Control);
        }

        // perform locks in the same order to avoid deadlocks
        let Ok(mut stream_inner) = self.lock() else {
            return Err(Closed);
        };
        let Ok(mut control_inner) = control.lock() else {
            return Err(Closed);
        };

        // make sure the stream hasn't been permanently closed
        ensure!(stream_inner.is_open, Err(Closed));
        ensure!(control_inner.is_open, Err(Closed));

        debug_assert!(
            !stream_inner.has_receiver && !control_inner.has_receiver,
            "receiver already open!\n stream: {stream_inner:?}\ncontrol: {control_inner:?}"
        );

        stream_inner.has_receiver = true;
        control_inner.has_receiver = true;

        Ok(())
    }

    #[inline]
    pub fn close_receiver(&self, control: &Self, half: Half) -> ControlFlow<()> {
        #[cfg(debug_assertions)]
        {
            assert_eq!(self.half, Half::Stream);
            assert_eq!(control.half, Half::Control);
        }

        // the Control half owns freeing in the case of poisoning
        let on_poisoned = if matches!(half, Half::Control) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        };

        // acquire both locks in the same order to avoid deadlocks or races
        let Ok(stream_inner) = self.lock() else {
            return on_poisoned;
        };
        let Ok(control_inner) = control.lock() else {
            return on_poisoned;
        };

        let (mut inner, other) = match half {
            Half::Stream => (stream_inner, control_inner),
            Half::Control => (control_inner, stream_inner),
        };

        debug_assert!(
            inner.has_receiver,
            "receiver already closed:\n{inner:?}\nother: {other:?}"
        );

        // observe the other half receiver status before dropping the `other` lock
        let has_other_receiver = other.has_receiver;
        drop(other);

        let wakers = inner.take_wakers();
        inner.has_receiver = false;
        // take the queue items out of the lock to avoid mutex poisoning.
        // note that most of the time this should be empty, which would be a no-op
        let mut queue = VecDeque::new();
        queue.append(&mut inner.queue);
        drop(inner);

        // drop wakers after the lock to avoid potential mutex poisoning
        wakers.dont_wake();

        if has_other_receiver {
            // the other queue still has the receiver don't put it back yet
            ControlFlow::Break(())
        } else {
            // we're the last receiver so free the queue
            ControlFlow::Continue(())
        }
    }

    #[inline]
    pub fn close(&self) {
        let Ok(mut inner) = self.lock() else {
            return;
        };
        inner.is_open = false;
        // Leave the remaining items in the queue in case the receiver wants them.

        // Notify the receiver that the queue is now closed
        let wakers = inner.take_wakers();
        drop(inner);
        drop(wakers);
    }

    #[inline]
    fn lock(&self) -> Result<std::sync::MutexGuard<Inner<T>>, Closed> {
        self.inner.lock().map_err(|_| Closed)
    }
}

struct Wakers {
    app_waker: Option<Waker>,
    worker_waker: Option<Waker>,
}

impl Wakers {
    #[inline]
    fn dont_wake(mut self) {
        self.app_waker = None;
        self.worker_waker = None;
    }
}

impl Drop for Wakers {
    #[inline]
    fn drop(&mut self) {
        if let Some(waker) = self.app_waker.take() {
            waker.wake();
        }
        if let Some(waker) = self.worker_waker.take() {
            waker.wake();
        }
    }
}
