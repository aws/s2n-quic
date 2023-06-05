// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::Gso,
    message::Message,
    socket::{ring::Consumer, task::events},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub trait Socket<T: Message> {
    type Error;

    fn send(
        &mut self,
        cx: &mut Context,
        entries: &mut [T],
        events: &mut events::TxEvents,
    ) -> Result<(), Self::Error>;
}

pub struct Sender<T: Message, S: Socket<T>> {
    ring: Consumer<T>,
    tx: S,
    pending: u32,
    events: events::TxEvents,
}

impl<T, S> Sender<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    #[inline]
    pub fn new(ring: Consumer<T>, tx: S, gso: Gso) -> Self {
        Self {
            ring,
            tx,
            pending: 0,
            events: events::TxEvents::new(gso),
        }
    }

    #[inline]
    fn poll_ring(&mut self, watermark: u32, cx: &mut Context) -> Poll<Option<usize>> {
        loop {
            let count = ready!(self.ring.poll_acquire(watermark, cx));

            // if we got more items than we have pending then yield
            if count > self.pending {
                return Some(self.pending as usize).into();
            }

            // release any of the pending items and try again
            self.release();
        }
    }

    #[inline]
    fn release(&mut self) {
        let to_release = core::mem::take(&mut self.pending);
        self.ring.release(to_release);
    }
}

impl<T, S> Future for Sender<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    type Output = Option<S::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        while !this.events.take_blocked() {
            let pending = match ready!(this.poll_ring(u32::MAX, cx)) {
                Some(entries) => entries,
                None => return None.into(),
            };

            // slice the ring data by the number of items we've already received
            let entries = &mut this.ring.data()[pending..];

            // perform the send syscall
            match this.tx.send(cx, entries, &mut this.events) {
                Ok(()) => {
                    // increment the number of received messages
                    this.pending += this.events.take_count() as u32
                }
                Err(err) => return Some(err).into(),
            }
        }

        // release any of the messages we wrote back to the consumer
        this.release();

        Poll::Pending
    }
}
