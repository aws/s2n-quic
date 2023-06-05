// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    message::Message,
    socket::{ring::Producer, task::events},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub trait Socket<T: Message> {
    type Error;

    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [T],
        events: &mut events::RxEvents,
    ) -> Result<(), Self::Error>;
}

pub struct Receiver<T: Message, S: Socket<T>> {
    ring: Producer<T>,
    rx: S,
    pending: u32,
}

impl<T, S> Receiver<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    #[inline]
    pub fn new(ring: Producer<T>, rx: S) -> Self {
        Self {
            ring,
            rx,
            pending: 0,
        }
    }

    #[inline]
    fn poll_ring(&mut self, watermark: u32, cx: &mut Context) -> Poll<Option<usize>> {
        loop {
            let count = ready!(self.ring.poll_acquire(watermark, cx));

            // if the number of items increased since last time then yield
            if count > self.pending {
                return Some(self.pending as usize).into();
            }

            // if we didn't get any items we can release the ones that we currently have and poll
            // again
            self.release();
        }
    }

    #[inline]
    fn release(&mut self) {
        let to_release = core::mem::take(&mut self.pending);
        self.ring.release(to_release);
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

        let mut events = events::RxEvents::default();

        while !events.take_blocked() {
            let pending = match ready!(this.poll_ring(u32::MAX, cx)) {
                Some(entries) => entries,
                None => return None.into(),
            };

            // slice the ring data by the number of items we've already received
            let entries = &mut this.ring.data()[pending..];

            // perform the recv syscall
            match this.rx.recv(cx, entries, &mut events) {
                Ok(()) => {
                    // increment the number of received messages
                    this.pending += events.take_count() as u32
                }
                Err(err) => return Some(err).into(),
            }
        }

        // release any of the messages we wrote back to the consumer
        this.release();

        Poll::Pending
    }
}
