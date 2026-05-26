// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! One half (stream or control) of a queue slot.
//!
//! A slot has two halves. Each half carries its own intrusive queue and a pair
//! of flags: `HAS_SENDER` (cleared by broadcast-close when the path secret is
//! evicted) and `HAS_RECEIVER` (cleared when the application-side handle is
//! dropped).

use crate::intrusive;
use bitflags::bitflags;
use core::{
    fmt,
    task::{Context, Poll, Waker},
};
use parking_lot::Mutex;

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Flags: u8 {
        /// The sender side is alive and may push entries.
        ///
        /// Cleared by `broadcast_close` when the path secret entry is evicted.
        /// Once cleared it is never re-set.
        const HAS_SENDER   = 0b01;
        /// A receiver handle exists for this half.
        ///
        /// Set by `open_receivers`, cleared when the application drops its handle.
        const HAS_RECEIVER = 0b10;
    }
}

pub(crate) struct HalfInner<T> {
    pub(crate) queue: intrusive::Queue<T>,
    pub(crate) flags: Flags,
    pub(crate) waker: Option<Waker>,
}

impl<T> HalfInner<T> {
    pub(crate) fn take_waker(&mut self) -> AutoWake {
        AutoWake(self.waker.take())
    }

    pub(crate) fn update_waker(&mut self, cx: &mut Context) {
        match &self.waker {
            Some(w) if w.will_wake(cx.waker()) => {}
            _ => self.waker = Some(cx.waker().clone()),
        }
    }
}

impl<T> fmt::Debug for HalfInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HalfInner")
            .field("queue_len", &self.queue.len())
            .field("flags", &self.flags)
            .field("has_waker", &self.waker.is_some())
            .finish()
    }
}

/// A token that wakes a stored waker when dropped.
#[derive(Default)]
pub struct AutoWake(pub(crate) Option<Waker>);

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
        if let Some(w) = self.0.take() {
            w.wake();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<T> {
    /// The slot is not bound to any receiver.
    Unallocated(T),
    /// The receiver side of this half has been dropped.
    HalfClosed(T),
    /// The sender was closed (path secret evicted).
    SenderClosed,
}

pub(crate) struct Half<T> {
    pub(crate) inner: Mutex<HalfInner<T>>,
}

impl<T> Half<T> {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(HalfInner {
                queue: intrusive::Queue::new(),
                flags: Flags::HAS_SENDER,
                waker: None,
            }),
        }
    }

    /// Push an entry into this half.
    ///
    /// Returns the stored waker on success so the caller can wake the receiver
    /// after releasing any outer lock.
    #[inline]
    pub(crate) fn push(
        &self,
        entry: intrusive::Entry<T>,
    ) -> Result<AutoWake, Error<intrusive::Entry<T>>> {
        let mut inner = self.inner.lock();

        if !inner.flags.contains(Flags::HAS_SENDER) {
            return Err(Error::SenderClosed);
        }
        if !inner.flags.contains(Flags::HAS_RECEIVER) {
            return Err(Error::HalfClosed(entry));
        }

        inner.queue.push_back(entry);
        Ok(inner.take_waker())
    }

    #[inline]
    pub(crate) fn pop(&self) -> Result<Option<intrusive::Entry<T>>, Closed> {
        let mut inner = self.inner.lock();
        if let Some(e) = inner.queue.pop_front() {
            return Ok(Some(e));
        }
        if !inner.flags.contains(Flags::HAS_SENDER) {
            return Err(Closed);
        }
        Ok(None)
    }

    #[inline]
    pub(crate) fn try_swap(&self) -> Result<intrusive::Queue<T>, Closed> {
        let mut inner = self.inner.lock();
        if inner.queue.is_empty() {
            if !inner.flags.contains(Flags::HAS_SENDER) {
                return Err(Closed);
            }
            return Ok(Default::default());
        }
        Ok(core::mem::take(&mut inner.queue))
    }

    #[inline]
    pub(crate) fn poll_pop(&self, cx: &mut Context) -> Poll<Result<intrusive::Entry<T>, Closed>> {
        let mut inner = self.inner.lock();
        if let Some(e) = inner.queue.pop_front() {
            return Poll::Ready(Ok(e));
        }
        if !inner.flags.contains(Flags::HAS_SENDER) {
            return Poll::Ready(Err(Closed));
        }
        inner.update_waker(cx);
        Poll::Pending
    }

    #[inline]
    pub(crate) fn poll_swap(
        &self,
        cx: &mut Context,
    ) -> Poll<Result<intrusive::Queue<T>, Closed>> {
        let mut inner = self.inner.lock();
        if !inner.queue.is_empty() {
            let q = core::mem::take(&mut inner.queue);
            if inner.flags.contains(Flags::HAS_SENDER) {
                inner.update_waker(cx);
            }
            return Poll::Ready(Ok(q));
        }
        if !inner.flags.contains(Flags::HAS_SENDER) {
            return Poll::Ready(Err(Closed));
        }
        inner.update_waker(cx);
        Poll::Pending
    }

    /// Broadcast-close: clear HAS_SENDER and return the stored waker.
    ///
    /// Does NOT push any data — receivers see a clean sender-gone state on
    /// their next poll.
    #[inline]
    pub(crate) fn broadcast_close(&self) -> AutoWake {
        let mut inner = self.inner.lock();
        inner.flags.remove(Flags::HAS_SENDER);
        inner.take_waker()
    }
}

/// Close one receiver half and potentially reclaim the slot.
///
/// Always acquires `stream` lock first then `control` (consistent order).
/// `closing_stream` selects which half is being closed.
/// When both halves are receiverless after the close, `on_last_receiver` is
/// called once (before returning `true`).
///
/// Returns `true` when this was the last receiver, signalling the caller to
/// reclaim the slot.
pub(crate) fn close_receiver<S, C, F>(
    stream: &Half<S>,
    control: &Half<C>,
    closing_stream: bool,
    on_last_receiver: F,
) -> bool
where
    F: FnOnce(),
{
    // Always lock stream → control to prevent deadlocks.
    let mut s = stream.inner.lock();
    let mut c = control.inner.lock();

    let (has_stream_rx, has_control_rx) = (
        s.flags.contains(Flags::HAS_RECEIVER),
        c.flags.contains(Flags::HAS_RECEIVER),
    );

    if closing_stream {
        debug_assert!(has_stream_rx, "stream receiver already closed");
        let waker = s.take_waker();
        s.flags.remove(Flags::HAS_RECEIVER);
        let queue = core::mem::take(&mut s.queue);
        let is_last = !has_control_rx;
        if is_last {
            on_last_receiver();
        }
        drop(c);
        drop(s);
        drop(waker);
        drop(queue);
        is_last
    } else {
        debug_assert!(has_control_rx, "control receiver already closed");
        let waker = c.take_waker();
        c.flags.remove(Flags::HAS_RECEIVER);
        let queue = core::mem::take(&mut c.queue);
        let is_last = !has_stream_rx;
        if is_last {
            on_last_receiver();
        }
        drop(c);
        drop(s);
        drop(waker);
        drop(queue);
        is_last
    }
}

impl<T> fmt::Debug for Half<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner.try_lock() {
            Some(inner) => fmt::Debug::fmt(&*inner, f),
            None => write!(f, "<locked>"),
        }
    }
}
