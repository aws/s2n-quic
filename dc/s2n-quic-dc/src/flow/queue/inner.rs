// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{intrusive_queue, sync::ring_deque::Closed};
use core::{
    fmt,
    ops::ControlFlow,
    task::{Context, Poll, Waker},
};
use s2n_quic_core::ensure;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<T> {
    /// The queue ID is not associated with a stream
    Unallocated(T),
    /// The queue exists but this half has no receiver
    HalfClosed(T),
    /// The entire queue is closed (either allocation dropped or key validation failed)
    FullyClosed(T),
    /// The sender has been dropped and no more packets will be sent
    PermanentlyClosed,
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

impl<T> From<Closed> for Error<T> {
    #[inline]
    fn from(_: Closed) -> Self {
        Self::PermanentlyClosed
    }
}

struct Inner<T> {
    queue: intrusive_queue::Queue<T>,
    is_open: bool,
    has_receiver: bool,
    waker: Option<Waker>,
}

impl<T> Inner<T> {
    fn take_waker(&mut self) -> Option<Waker> {
        self.waker.take()
    }

    fn update_waker(&mut self, cx: &mut Context) {
        // Only clone waker if it's different from the current one
        if let Some(ref waker) = self.waker {
            if !waker.will_wake(cx.waker()) {
                self.waker = Some(cx.waker().clone());
            }
        } else {
            self.waker = Some(cx.waker().clone());
        }
    }
}

impl<T> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("is_empty", &self.queue.is_empty())
            .field("is_open", &self.is_open)
            .field("has_receiver", &self.has_receiver)
            .field("has_waker", &self.waker.is_some())
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
    pub fn new(half: Half) -> Self {
        Self {
            inner: Mutex::new(Inner {
                queue: intrusive_queue::Queue::new(),
                is_open: true,
                has_receiver: false,
                waker: None,
            }),
            #[cfg(debug_assertions)]
            half,
        }
    }

    #[inline]
    pub fn push<F>(
        &self,
        entry: intrusive_queue::Entry<T>,
        validate: F,
    ) -> Result<(), Error<intrusive_queue::Entry<T>>>
    where
        F: FnOnce() -> bool,
    {
        let mut inner = self.lock()?;
        // check if the queue is permanently closed (sender dropped)
        ensure!(inner.is_open, Err(Error::PermanentlyClosed));
        // check if this half has a receiver
        ensure!(inner.has_receiver, Err(Error::HalfClosed(entry)));
        // validate key inside the lock - if this fails, the entire flow is invalid
        ensure!(validate(), Err(Error::FullyClosed(entry)));

        inner.queue.push_back(entry);
        let waker = inner.take_waker();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }

        Ok(())
    }

    #[inline]
    pub fn pop(&self) -> Result<Option<intrusive_queue::Entry<T>>, Closed> {
        let mut inner = self.lock()?;
        if let Some(entry) = inner.queue.pop_front() {
            return Ok(Some(entry));
        }

        ensure!(inner.is_open, Err(Closed));
        Ok(None)
    }

    #[inline]
    pub fn poll_pop(&self, cx: &mut Context) -> Poll<Result<intrusive_queue::Entry<T>, Closed>> {
        let mut inner = self.lock()?;
        if let Some(entry) = inner.queue.pop_front() {
            return Ok(entry).into();
        }

        ensure!(inner.is_open, Err(Closed).into());

        inner.update_waker(cx);

        Poll::Pending
    }

    #[inline]
    pub fn poll_swap(&self, cx: &mut Context) -> Poll<Result<intrusive_queue::Queue<T>, Closed>> {
        let mut inner = self.lock()?;

        if inner.queue.is_empty() {
            ensure!(inner.is_open, Err(Closed).into());
            inner.update_waker(cx);
            return Poll::Pending;
        }

        let queue = core::mem::take(&mut inner.queue);
        let is_open = inner.is_open;

        // Always update waker since we drained everything (if still open)
        if is_open {
            inner.update_waker(cx);
        }

        drop(inner);

        // If queue was closed, wake immediately to process the closed state
        if !is_open {
            cx.waker().wake_by_ref();
        }

        Ok(queue).into()
    }

    #[inline]
    pub fn open_receivers<C>(&self, control: &Queue<C>) -> Result<(), Closed> {
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
    pub fn close_receiver<C>(&self, control: &Queue<C>, half: Half) -> ControlFlow<()> {
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

        match half {
            Half::Stream => Self::close_receiver_inner(stream_inner, control_inner),
            Half::Control => Self::close_receiver_inner(control_inner, stream_inner),
        }
    }

    fn close_receiver_inner<Closing, Other>(
        mut closing: std::sync::MutexGuard<'_, Inner<Closing>>,
        other: std::sync::MutexGuard<'_, Inner<Other>>,
    ) -> ControlFlow<()> {
        debug_assert!(
            closing.has_receiver,
            "receiver already closed:\n{closing:?}\nother: {other:?}"
        );

        // observe the other half receiver status before dropping the `other` lock
        let has_other_receiver = other.has_receiver;
        drop(other);

        let waker = closing.take_waker();
        closing.has_receiver = false;
        // take the queue items out of the lock to avoid mutex poisoning.
        // note that most of the time this should be empty, which would be a no-op
        let queue = core::mem::take(&mut closing.queue);
        drop(closing);

        // Don't wake the waker - receiver is closing
        drop(waker);
        // Drop the queue entries
        drop(queue);

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
        let waker = inner.take_waker();
        drop(inner);
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Validates the queue has a receiver (is allocated) and invokes the closure with a validation result.
    ///
    /// Returns Ok(R) if the queue has a receiver, Err(()) otherwise.
    #[inline]
    pub fn with_key<F, R>(&self, f: F) -> Result<R, ()>
    where
        F: FnOnce() -> R,
    {
        let inner = self.lock().map_err(|_| ())?;

        // Check if the queue has a receiver (is allocated)
        if !inner.has_receiver {
            return Err(());
        }

        // Execute the closure while holding the lock
        let result = f();
        drop(inner);
        Ok(result)
    }

    #[inline]
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner<T>>, Closed> {
        self.inner.lock().map_err(|_| Closed)
    }
}
