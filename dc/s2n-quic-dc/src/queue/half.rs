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
    pub(crate) fn poll_swap(&self, cx: &mut Context) -> Poll<Result<intrusive::Queue<T>, Closed>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{endpoint::msg, queue::testing::*};
    use core::task::Poll;
    use std::sync::Arc;

    fn open_half() -> Half<msg::Stream> {
        let half = Half::new();
        half.inner.lock().flags.insert(Flags::HAS_RECEIVER);
        half
    }

    fn push_ok(half: &Half<msg::Stream>, entry: crate::intrusive::Entry<msg::Stream>) -> AutoWake {
        let mut inner = half.inner.lock();
        inner.queue.push_back(entry);
        inner.take_waker()
    }

    #[test]
    fn pop_fifo_order() {
        let half = open_half();
        for i in 0..3u8 {
            let entry = crate::intrusive::Entry::new(msg::Stream::Data {
                offset: s2n_quic_core::varint::VarInt::from_u8(i),
                fin: false,
                payload: bytes::BytesMut::from(&[i][..]),
            });
            push_ok(&half, entry);
        }

        for i in 0..3u8 {
            let entry = half.pop().unwrap().unwrap();
            match &*entry {
                msg::Stream::Data { offset, .. } => {
                    assert_eq!(offset.as_u64(), i as u64);
                }
                _ => panic!("unexpected variant"),
            }
        }
    }

    #[test]
    fn pop_empty_open_returns_none() {
        let half = open_half();
        assert!(matches!(half.pop(), Ok(None)));
    }

    #[test]
    fn pop_empty_closed_returns_err() {
        let half = open_half();
        half.broadcast_close();
        assert!(matches!(half.pop(), Err(Closed)));
    }

    #[test]
    fn pop_drains_remaining_after_close() {
        let half = open_half();
        push_ok(&half, make_stream_entry());
        half.broadcast_close();
        assert!(half.pop().unwrap().is_some());
        assert!(matches!(half.pop(), Err(Closed)));
    }

    #[test]
    fn try_swap_returns_full_queue() {
        let half = open_half();
        push_ok(&half, make_stream_entry());
        push_ok(&half, make_stream_entry());
        let q = half.try_swap().unwrap();
        assert_eq!(q.len(), 2);
        let q2 = half.try_swap().unwrap();
        assert_eq!(q2.len(), 0);
    }

    #[test]
    fn try_swap_after_close_empty_returns_closed() {
        let half = open_half();
        half.broadcast_close();
        assert!(matches!(half.try_swap(), Err(Closed)));
    }

    #[test]
    fn poll_pop_pending_when_empty() {
        let half = open_half();
        let (waker, count) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_pop(&mut cx), Poll::Pending));
        assert!(half.inner.lock().waker.is_some());
        assert_eq!(count.load(core::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn poll_pop_ready_with_data() {
        let half = open_half();
        push_ok(&half, make_stream_entry());
        let (waker, _) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_pop(&mut cx), Poll::Ready(Ok(_))));
    }

    #[test]
    fn poll_pop_ready_closed() {
        let half = open_half();
        half.broadcast_close();
        let (waker, _) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_pop(&mut cx), Poll::Ready(Err(Closed))));
    }

    #[test]
    fn poll_swap_pending_when_empty() {
        let half = open_half();
        let (waker, _) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_swap(&mut cx), Poll::Pending));
        assert!(half.inner.lock().waker.is_some());
    }

    #[test]
    fn poll_swap_ready_with_data_registers_waker() {
        let half = open_half();
        push_ok(&half, make_stream_entry());
        let (waker, _) = test_waker();
        let mut cx = test_context(&waker);
        let result = half.poll_swap(&mut cx);
        assert!(matches!(result, Poll::Ready(Ok(_))));
        // Sender alive → waker re-registered for next batch
        assert!(half.inner.lock().waker.is_some());
    }

    #[test]
    fn auto_wake_fires_on_drop() {
        let (waker, count) = test_waker();
        let aw = AutoWake(Some(waker));
        drop(aw);
        assert_eq!(count.load(core::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn auto_wake_take_prevents_fire() {
        let (waker, count) = test_waker();
        let mut aw = AutoWake(Some(waker));
        let _ = aw.take();
        drop(aw);
        assert_eq!(count.load(core::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn broadcast_close_returns_stored_waker() {
        let half = open_half();
        let (waker, count) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_pop(&mut cx), Poll::Pending));
        let aw = half.broadcast_close();
        assert!(aw.is_some());
        drop(aw);
        assert_eq!(count.load(core::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn push_wakes_stored_waker() {
        let half = open_half();
        let (waker, count) = test_waker();
        let mut cx = test_context(&waker);
        assert!(matches!(half.poll_pop(&mut cx), Poll::Pending));
        let aw = push_ok(&half, make_stream_entry());
        drop(aw);
        assert_eq!(count.load(core::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn waker_replaced_on_different_waker() {
        let half = open_half();
        let (w1, count1) = test_waker();
        let (w2, count2) = test_waker();

        let mut cx1 = test_context(&w1);
        assert!(matches!(half.poll_pop(&mut cx1), Poll::Pending));

        let mut cx2 = test_context(&w2);
        assert!(matches!(half.poll_pop(&mut cx2), Poll::Pending));

        let aw = push_ok(&half, make_stream_entry());
        drop(aw);
        // W2 should fire, not W1
        assert_eq!(count1.load(core::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(count2.load(core::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn close_stream_first_not_last() {
        let stream = Half::<msg::Stream>::new();
        let control = Half::<msg::Control>::new();
        {
            let mut s = stream.inner.lock();
            s.flags.insert(Flags::HAS_RECEIVER);
        }
        {
            let mut c = control.inner.lock();
            c.flags.insert(Flags::HAS_RECEIVER);
        }
        let mut called = false;
        let is_last = close_receiver(&stream, &control, true, || called = true);
        assert!(!is_last);
        assert!(!called);
        assert!(!stream.inner.lock().flags.contains(Flags::HAS_RECEIVER));
        assert!(control.inner.lock().flags.contains(Flags::HAS_RECEIVER));
    }

    #[test]
    fn close_control_first_not_last() {
        let stream = Half::<msg::Stream>::new();
        let control = Half::<msg::Control>::new();
        {
            let mut s = stream.inner.lock();
            s.flags.insert(Flags::HAS_RECEIVER);
        }
        {
            let mut c = control.inner.lock();
            c.flags.insert(Flags::HAS_RECEIVER);
        }
        let mut called = false;
        let is_last = close_receiver(&stream, &control, false, || called = true);
        assert!(!is_last);
        assert!(!called);
        assert!(stream.inner.lock().flags.contains(Flags::HAS_RECEIVER));
        assert!(!control.inner.lock().flags.contains(Flags::HAS_RECEIVER));
    }

    #[test]
    fn close_stream_last_calls_on_last() {
        let stream = Half::<msg::Stream>::new();
        let control = Half::<msg::Control>::new();
        {
            let mut s = stream.inner.lock();
            s.flags.insert(Flags::HAS_RECEIVER);
        }
        // control has no HAS_RECEIVER
        let mut called = false;
        let is_last = close_receiver(&stream, &control, true, || called = true);
        assert!(is_last);
        assert!(called);
    }

    #[test]
    fn close_control_last_calls_on_last() {
        let stream = Half::<msg::Stream>::new();
        let control = Half::<msg::Control>::new();
        {
            let mut c = control.inner.lock();
            c.flags.insert(Flags::HAS_RECEIVER);
        }
        // stream has no HAS_RECEIVER
        let mut called = false;
        let is_last = close_receiver(&stream, &control, false, || called = true);
        assert!(is_last);
        assert!(called);
    }

    #[test]
    fn close_receiver_drains_queue() {
        let stream = Half::<msg::Stream>::new();
        let control = Half::<msg::Control>::new();
        {
            let mut s = stream.inner.lock();
            s.flags.insert(Flags::HAS_RECEIVER);
            s.queue.push_back(make_stream_entry());
            s.queue.push_back(make_stream_entry());
        }
        close_receiver(&stream, &control, true, || {});
        assert_eq!(stream.inner.lock().queue.len(), 0);
    }

    #[test]
    fn push_wakes_blocked_poll_pop() {
        use crate::testing::{ext::*, sim};

        sim(|| {
            let half = Arc::new(open_half());
            let half2 = half.clone();

            async move {
                let result = core::future::poll_fn(|cx| half.poll_pop(cx)).await;
                assert!(result.is_ok());
            }
            .primary()
            .spawn();

            async move {
                bach::task::yield_now().await;
                let aw = push_ok(&half2, make_stream_entry());
                drop(aw);
            }
            .primary()
            .spawn();
        });
    }

    #[test]
    fn broadcast_close_wakes_pending_receiver() {
        use crate::testing::{ext::*, sim};

        sim(|| {
            let half = Arc::new(open_half());
            let half2 = half.clone();

            async move {
                let result = core::future::poll_fn(|cx| half.poll_pop(cx)).await;
                assert!(matches!(result, Err(Closed)));
            }
            .primary()
            .spawn();

            async move {
                bach::task::yield_now().await;
                let aw = half2.broadcast_close();
                drop(aw);
            }
            .primary()
            .spawn();
        });
    }
}
