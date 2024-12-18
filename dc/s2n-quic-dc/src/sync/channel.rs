// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::ring_deque::{self, RingDeque};
use core::{fmt, marker::PhantomPinned, pin::Pin, task::Poll};
use event_listener_strategy::{
    easy_wrapper,
    event_listener::{Event, EventListener},
    EventListenerFuture, Strategy,
};
use pin_project_lite::pin_project;
use s2n_quic_core::ready;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Weak,
};

pub use ring_deque::{Closed, Priority};

pub fn new<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    assert!(cap >= 1, "capacity must be at least 2");

    let channel = Arc::new(Channel {
        queue: RingDeque::new(cap),
        recv_ops: Event::new(),
        sender_count: AtomicUsize::new(1),
        receiver_count: AtomicUsize::new(1),
    });

    let s = Sender {
        channel: channel.clone(),
    };
    let r = Receiver {
        listener: None,
        channel,
        _pin: PhantomPinned,
    };
    (s, r)
}

struct Channel<T> {
    queue: RingDeque<T>,
    recv_ops: Event,
    sender_count: AtomicUsize,
    receiver_count: AtomicUsize,
}

impl<T> Channel<T> {
    /// Closes the channel and notifies all blocked operations.
    ///
    /// Returns `Err` if this call has closed the channel and it was not closed already.
    fn close(&self) -> Result<(), Closed> {
        self.queue.close()?;

        // Notify all receive and send operations.
        self.recv_ops.notify(usize::MAX);

        Ok(())
    }
}

pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    #[inline]
    pub fn send_back(&self, msg: T) -> Result<Option<T>, Closed> {
        let res = self.channel.queue.push_back(msg)?;

        // Notify a blocked receive operation. If the notified operation gets canceled,
        // it will notify another blocked receive operation.
        self.channel.recv_ops.notify_additional(1);

        Ok(res)
    }

    #[inline]
    pub fn send_front(&self, msg: T) -> Result<Option<T>, Closed> {
        let res = self.channel.queue.push_front(msg)?;

        // Notify a blocked receive operation. If the notified operation gets canceled,
        // it will notify another blocked receive operation.
        self.channel.recv_ops.notify_additional(1);

        Ok(res)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Decrement the sender count and close the channel if it drops down to zero.
        if self.channel.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.channel.close();
        }
    }
}

impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sender {{ .. }}")
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        let count = self.channel.sender_count.fetch_add(1, Ordering::Relaxed);

        // Make sure the count never overflows, even if lots of sender clones are leaked.
        assert!(count < usize::MAX / 2, "too many senders");

        Sender {
            channel: self.channel.clone(),
        }
    }
}

pin_project! {
    /// The receiving side of a channel.
    ///
    /// Receivers can be cloned and shared among threads. When all receivers associated with a channel
    /// are dropped, the channel becomes closed.
    ///
    /// The channel can also be closed manually by calling [`Receiver::close()`].
    ///
    /// Receivers implement the [`Stream`] trait.
    pub struct Receiver<T> {
        // Inner channel state.
        channel: Arc<Channel<T>>,

        // Listens for a send or close event to unblock this stream.
        listener: Option<EventListener>,

        // Keeping this type `!Unpin` enables future optimizations.
        #[pin]
        _pin: PhantomPinned
    }

    impl<T> PinnedDrop for Receiver<T> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();

            // Decrement the receiver count and close the channel if it drops down to zero.
            if this.channel.receiver_count.fetch_sub(1, Ordering::AcqRel) == 1 {
                let _ = this.channel.close();
            }
        }
    }
}

#[allow(dead_code)] // TODO remove this once the module is public
impl<T> Receiver<T> {
    /// Attempts to receive a message from the front of the channel.
    ///
    /// If the channel is empty, or empty and closed, this method returns an error.
    #[inline]
    pub fn try_recv_front(&self) -> Result<Option<T>, Closed> {
        self.channel.queue.pop_front()
    }

    /// Attempts to receive a message from the back of the channel.
    ///
    /// If the channel is empty, or empty and closed, this method returns an error.
    #[inline]
    pub fn try_recv_back(&self) -> Result<Option<T>, Closed> {
        self.channel.queue.pop_back()
    }

    /// Receives a message from the front of the channel.
    ///
    /// If the channel is empty, this method waits until there is a message.
    ///
    /// If the channel is closed, this method receives a message or returns an error if there are
    /// no more messages.
    #[inline]
    pub fn recv_front(&self) -> Recv<'_, T> {
        Recv::_new(RecvInner {
            receiver: self,
            pop_end: PopEnd::Front,
            listener: None,
            _pin: PhantomPinned,
        })
    }

    /// Receives a message from the back of the channel.
    ///
    /// If the channel is empty, this method waits until there is a message.
    ///
    /// If the channel is closed, this method receives a message or returns an error if there are
    /// no more messages.
    #[inline]
    pub fn recv_back(&self) -> Recv<'_, T> {
        Recv::_new(RecvInner {
            receiver: self,
            pop_end: PopEnd::Back,
            listener: None,
            _pin: PhantomPinned,
        })
    }

    #[inline]
    pub fn downgrade(&self) -> WeakReceiver<T> {
        WeakReceiver {
            channel: Arc::downgrade(&self.channel),
        }
    }
}

impl<T> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Receiver {{ .. }}")
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        let count = self.channel.receiver_count.fetch_add(1, Ordering::Relaxed);

        // Make sure the count never overflows, even if lots of receiver clones are leaked.
        assert!(count < usize::MAX / 2);

        Receiver {
            channel: self.channel.clone(),
            listener: None,
            _pin: PhantomPinned,
        }
    }
}

#[derive(Clone)]
pub struct WeakReceiver<T> {
    channel: Weak<Channel<T>>,
}

#[allow(dead_code)] // TODO remove this once the module is public
impl<T> WeakReceiver<T> {
    #[inline]
    pub fn pop_front_if<F>(&self, priority: Priority, f: F) -> Result<Option<T>, Closed>
    where
        F: FnOnce(&T) -> bool,
    {
        let channel = self.channel.upgrade().ok_or(Closed)?;
        channel.queue.pop_front_if(priority, f)
    }

    #[inline]
    pub fn pop_back_if<F>(&self, priority: Priority, f: F) -> Result<Option<T>, Closed>
    where
        F: FnOnce(&T) -> bool,
    {
        let channel = self.channel.upgrade().ok_or(Closed)?;
        channel.queue.pop_back_if(priority, f)
    }
}

easy_wrapper! {
    /// A future returned by [`Receiver::recv()`].
    #[derive(Debug)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct Recv<'a, T>(RecvInner<'a, T> => Result<T, Closed>);
    pub(crate) wait();
}

#[derive(Debug)]
enum PopEnd {
    Front,
    Back,
}

pin_project! {
    #[derive(Debug)]
    #[project(!Unpin)]
    struct RecvInner<'a, T> {
        // Reference to the receiver.
        receiver: &'a Receiver<T>,

        pop_end: PopEnd,

        // Listener waiting on the channel.
        listener: Option<EventListener>,

        // Keeping this type `!Unpin` enables future optimizations.
        #[pin]
        _pin: PhantomPinned
    }
}

impl<T> EventListenerFuture for RecvInner<'_, T> {
    type Output = Result<T, Closed>;

    /// Run this future with the given `Strategy`.
    fn poll_with_strategy<'x, S: Strategy<'x>>(
        self: Pin<&mut Self>,
        strategy: &mut S,
        cx: &mut S::Context,
    ) -> Poll<Result<T, Closed>> {
        let this = self.project();

        loop {
            // Attempt to receive a message.
            let message = match this.pop_end {
                PopEnd::Front => this.receiver.try_recv_front(),
                PopEnd::Back => this.receiver.try_recv_back(),
            }?;
            if let Some(msg) = message {
                return Poll::Ready(Ok(msg));
            }

            // Receiving failed - now start listening for notifications or wait for one.
            if this.listener.is_some() {
                // Poll using the given strategy
                ready!(S::poll(strategy, &mut *this.listener, cx));
            } else {
                *this.listener = Some(this.receiver.channel.recv_ops.listen());
            }
        }
    }
}
