// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    message::Message,
    socket::{ring::Producer, stats, task::events},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::task::cooldown::Cooldown;

pub use events::RxEvents as Events;

pub trait Socket<T: Message> {
    type Error;

    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [T],
        events: &mut Events,
        stats: &stats::Sender,
    ) -> Result<(), Self::Error>;
}

pub struct Receiver<T: Message, S: Socket<T>> {
    ring: Producer<T>,
    /// Implementation of a socket that fills free slots in the ring buffer
    rx: S,
    ring_cooldown: Cooldown,
    io_cooldown: Cooldown,
    stats: stats::Sender,
    has_registered_drop_waker: bool,
}

impl<T, S> Receiver<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    #[inline]
    pub fn new(ring: Producer<T>, rx: S, cooldown: Cooldown, stats: stats::Sender) -> Self {
        Self {
            ring,
            rx,
            ring_cooldown: cooldown.clone(),
            io_cooldown: cooldown,
            stats,
            has_registered_drop_waker: false,
        }
    }

    #[inline]
    fn poll_ring(&mut self, watermark: u32, cx: &mut Context) -> Poll<Result<(), ()>> {
        loop {
            let is_loop = self.ring_cooldown.state().is_loop();

            let count = if is_loop {
                self.ring.acquire(watermark)
            } else {
                match self.ring.poll_acquire(watermark, cx) {
                    Poll::Ready(count) => count,
                    Poll::Pending if !self.ring.is_open() => return Err(()).into(),
                    Poll::Pending => 0,
                }
            };

            // if the number of free slots increased since last time then yield
            if count > 0 {
                self.ring_cooldown.on_ready();
                return Ok(()).into();
            }

            if is_loop && self.ring_cooldown.on_pending_task(cx).is_sleep() {
                continue;
            }

            return Poll::Pending;
        }
    }
}

impl<T, S> Future for Receiver<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    type Output = Option<S::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        if !this.has_registered_drop_waker {
            this.has_registered_drop_waker = true;
            this.ring.register_drop_waker(cx);
        }

        let mut events = Events::default();

        let mut pending_wake = false;

        while !events.take_blocked() {
            match this.poll_ring(u32::MAX, cx) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_)) => return None.into(),
                Poll::Pending => {
                    if pending_wake {
                        this.ring.wake();
                    }
                    return Poll::Pending;
                }
            }

            let entries = this.ring.data();

            // perform the recv syscall
            match this.rx.recv(cx, entries, &mut events, &this.stats) {
                Ok(_) => {
                    // increment the number of received messages
                    let count = events.take_count() as u32;

                    if count > 0 {
                        this.ring.release_no_wake(count);
                        this.io_cooldown.on_ready();
                        pending_wake = true;
                    }
                }
                Err(err) => return Some(err).into(),
            }
        }

        this.io_cooldown.on_pending_task(cx);

        if pending_wake {
            this.ring.wake();
        }

        if !this.ring.is_open() {
            return Poll::Ready(None);
        }
        Poll::Pending
    }
}
