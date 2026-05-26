// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::intrusive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closed;
use bitflags::bitflags;
use core::{
    fmt,
    ops::ControlFlow,
    task::{Context, Poll, Waker},
};
use parking_lot::{Mutex, MutexGuard};
use s2n_quic_core::ensure;

bitflags! {
    /// Packed state flags for a queue half, stored in a single byte.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Flags: u8 {
        /// The sender side is still alive and may push entries.
        ///
        /// If this is not set, the sender is permanently gone and will not open again.
        const IS_OPEN = 0b0001;
        /// A receiver handle exists for this queue half.
        ///
        /// If this is not set, the receiver has dropped its handle and it has released it
        /// back into the descriptor pool.
        const HAS_RECEIVER = 0b0010;
        /// At least one message has been successfully pushed since allocation.
        ///
        /// Used to trigger a one-time relaxed store of the remote queue ID in
        /// the descriptor — after this is set the dispatcher skips the write.
        const HAS_OBSERVED = 0b0100;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<T> {
    /// The queue ID is not associated with a stream
    Unallocated(T),
    /// The queue exists but this half has no receiver
    HalfClosed(T),
    /// The queue key validation failed (credential or binding_id mismatch)
    ValidationFailed(T, super::descriptor::ValidationError),
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
    queue: intrusive::Queue<T>,
    flags: Flags,
    waker: Option<Waker>,
}

impl<T> Inner<T> {
    fn take_waker(&mut self) -> AutoWake {
        AutoWake(self.waker.take())
    }

    fn update_waker(&mut self, cx: &mut Context) {
        if let Some(ref waker) = self.waker {
            if !waker.will_wake(cx.waker()) {
                self.waker = Some(cx.waker().clone());
            }
        } else {
            self.waker = Some(cx.waker().clone());
        }
    }
}

#[derive(Default)]
pub struct AutoWake(Option<Waker>);

impl AutoWake {
    pub fn new(waker: Option<Waker>) -> Self {
        Self(waker)
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn take(&mut self) -> Option<Waker> {
        self.0.take()
    }
}

impl Drop for AutoWake {
    fn drop(&mut self) {
        if let Some(waker) = self.0.take() {
            waker.wake();
        }
    }
}

impl<T> fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("is_empty", &self.queue.is_empty())
            .field("flags", &self.flags)
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
                queue: intrusive::Queue::new(),
                flags: Flags::IS_OPEN,
                waker: None,
            }),
            #[cfg(debug_assertions)]
            half,
        }
    }

    /// Push an entry into the queue.
    ///
    /// `observe` is called inside the lock when `HAS_OBSERVED` is not yet set. If it
    /// returns `true`, the flag is set and the caller should have performed any
    /// side-effects (e.g. storing the remote queue ID) within the callback. This
    /// guarantees the side-effect is visible before the receiver can pop the entry.
    #[inline]
    pub fn push<F, O>(
        &self,
        entry: intrusive::Entry<T>,
        observe: O,
        validate: F,
    ) -> Result<AutoWake, Error<intrusive::Entry<T>>>
    where
        F: FnOnce() -> Result<(), super::descriptor::ValidationError>,
        O: FnOnce() -> bool,
    {
        let mut inner = self.lock()?;
        ensure!(
            inner.flags.contains(Flags::IS_OPEN),
            Err(Error::PermanentlyClosed)
        );
        ensure!(
            inner.flags.contains(Flags::HAS_RECEIVER),
            Err(Error::HalfClosed(entry))
        );
        if let Err(reason) = validate() {
            return Err(Error::ValidationFailed(entry, reason));
        }

        if !inner.flags.contains(Flags::HAS_OBSERVED) && observe() {
            inner.flags.insert(Flags::HAS_OBSERVED);
        }

        inner.queue.push_back(entry);
        let waker = inner.take_waker();
        drop(inner);

        Ok(waker)
    }

    #[inline]
    pub fn pop(&self) -> Result<Option<intrusive::Entry<T>>, Closed> {
        let mut inner = self.lock()?;
        if let Some(entry) = inner.queue.pop_front() {
            return Ok(Some(entry));
        }

        ensure!(inner.flags.contains(Flags::IS_OPEN), Err(Closed));
        Ok(None)
    }

    #[inline]
    pub fn try_swap(&self) -> Result<intrusive::Queue<T>, Closed> {
        let mut inner = self.lock()?;

        if inner.queue.is_empty() {
            ensure!(inner.flags.contains(Flags::IS_OPEN), Err(Closed));
            return Ok(Default::default());
        }

        Ok(core::mem::take(&mut inner.queue))
    }

    #[inline]
    pub fn poll_pop(&self, cx: &mut Context) -> Poll<Result<intrusive::Entry<T>, Closed>> {
        let mut inner = self.lock()?;
        if let Some(entry) = inner.queue.pop_front() {
            return Ok(entry).into();
        }

        ensure!(inner.flags.contains(Flags::IS_OPEN), Err(Closed).into());

        inner.update_waker(cx);

        Poll::Pending
    }

    #[inline]
    pub fn poll_swap(&self, cx: &mut Context) -> Poll<Result<intrusive::Queue<T>, Closed>> {
        let mut inner = self.lock()?;

        if inner.queue.is_empty() {
            ensure!(inner.flags.contains(Flags::IS_OPEN), Err(Closed).into());
            inner.update_waker(cx);
            return Poll::Pending;
        }

        let queue = core::mem::take(&mut inner.queue);
        let is_open = inner.flags.contains(Flags::IS_OPEN);

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

    /// Opens both receiver halves for this queue pair.
    ///
    /// If `has_remote_queue_id` is true, both halves are marked as already observed so
    /// the dispatcher will skip the one-time remote queue ID store (it was set at alloc time).
    #[inline]
    pub fn open_receivers<C>(
        &self,
        control: &Queue<C>,
        has_remote_queue_id: bool,
    ) -> Result<(), Closed> {
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

        ensure!(stream_inner.flags.contains(Flags::IS_OPEN), Err(Closed));
        ensure!(control_inner.flags.contains(Flags::IS_OPEN), Err(Closed));

        debug_assert!(
            !stream_inner.flags.contains(Flags::HAS_RECEIVER)
                && !control_inner.flags.contains(Flags::HAS_RECEIVER),
            "receiver already open!\n stream: {stream_inner:?}\ncontrol: {control_inner:?}"
        );

        let mut open_flags = Flags::HAS_RECEIVER;
        if has_remote_queue_id {
            open_flags |= Flags::HAS_OBSERVED;
        }

        stream_inner.flags.insert(open_flags);
        control_inner.flags.insert(open_flags);

        Ok(())
    }

    #[inline]
    pub fn close_receiver<C, F>(
        &self,
        control: &Queue<C>,
        half: Half,
        on_last_receiver: F,
    ) -> ControlFlow<()>
    where
        F: FnOnce(),
    {
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
            Half::Stream => {
                Self::close_receiver_inner(stream_inner, control_inner, on_last_receiver)
            }
            Half::Control => {
                Self::close_receiver_inner(control_inner, stream_inner, on_last_receiver)
            }
        }
    }

    fn close_receiver_inner<Closing, Other, F>(
        mut closing: MutexGuard<'_, Inner<Closing>>,
        other: MutexGuard<'_, Inner<Other>>,
        on_last_receiver: F,
    ) -> ControlFlow<()>
    where
        F: FnOnce(),
    {
        debug_assert!(
            closing.flags.contains(Flags::HAS_RECEIVER),
            "receiver already closed:\n{closing:?}\nother: {other:?}"
        );

        // observe the other half receiver status while holding both locks
        let has_other_receiver = other.flags.contains(Flags::HAS_RECEIVER);

        let waker = closing.take_waker();
        // Clear both HAS_RECEIVER and HAS_OBSERVED so recycled descriptors get fresh state
        closing
            .flags
            .remove(Flags::HAS_RECEIVER | Flags::HAS_OBSERVED);

        if !has_other_receiver {
            on_last_receiver();
        }

        drop(other);
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
    pub fn close(&self) -> AutoWake {
        let Ok(mut inner) = self.lock() else {
            return AutoWake(None);
        };
        inner.flags.remove(Flags::IS_OPEN);

        // Notify the receiver that the queue is now closed
        let waker = inner.take_waker();
        drop(inner);
        waker
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
        if !inner.flags.contains(Flags::HAS_RECEIVER) {
            return Err(());
        }

        // Execute the closure while holding the lock
        let result = f();
        drop(inner);
        Ok(result)
    }

    #[inline]
    fn lock(&self) -> Result<MutexGuard<'_, Inner<T>>, Closed> {
        Ok(self.inner.lock()) // .map_err(|_| Closed)
    }
}
