// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{ring, syscall, umem::Umem};
use core::task::{Context, Poll};
use s2n_codec::{Encoder as _, EncoderBuffer};
use s2n_quic_core::{
    event,
    io::tx,
    sync::atomic_waker,
    xdp::{encoder, path},
};

/// Drives the Tx and Completion rings forward
pub trait Driver: 'static {
    #[inline]
    fn poll(
        &mut self,
        tx: &mut ring::Tx,
        completion: &mut ring::Completion,
        cx: &mut Context,
    ) -> Option<bool> {
        // Default to doing nothing
        //
        // In order to keep the trait signature from having `_` prefixes in the name, discard the
        // variables in the body.
        let _ = tx;
        let _ = completion;
        let _ = cx;
        Some(false)
    }

    #[inline]
    fn wake(&mut self, tx: &mut ring::Tx, completion: &mut ring::Completion) {
        // Default to doing nothing
        //
        // In order to keep the trait signature from having `_` prefixes in the name, discard the
        // variables in the body.
        let _ = tx;
        let _ = completion;
    }
}

impl Driver for () {}

impl Driver for atomic_waker::Handle {
    #[inline]
    fn poll(
        &mut self,
        tx: &mut ring::Tx,
        completion: &mut ring::Completion,
        cx: &mut Context,
    ) -> Option<bool> {
        // record if either of the rings were empty to start
        let was_empty = tx.is_empty() || completion.is_empty();

        // iterate twice to avoid waker registration races
        for i in 0..2 {
            let count = completion.acquire(u32::MAX);
            let count = tx.acquire(count).min(count);

            trace!("acquired {count} entries");

            // return if we have entries in both rings
            if count > 0 {
                return Some(was_empty);
            }

            // if the peer's handle is closed, then shut down the task
            if !self.is_open() {
                return None;
            }

            // only register wakers if it's the first iteration and it started out empty
            if i > 0 || !was_empty {
                continue;
            }

            trace!("registering waker");
            self.register(cx.waker());
            trace!("waking waker");
            self.wake(tx, completion);
        }

        // we need to keep polling until we have at least one item here
        if tx.needs_wakeup() || completion.is_empty() || tx.is_empty() {
            atomic_waker::Handle::wake(self);
        }

        Some(false)
    }

    #[inline]
    fn wake(&mut self, tx: &mut ring::Tx, _completion: &mut ring::Completion) {
        if tx.needs_wakeup() {
            atomic_waker::Handle::wake(self);
        }
    }
}

pub struct BusyPoll;

impl Driver for BusyPoll {
    #[inline]
    fn poll(
        &mut self,
        tx: &mut ring::Tx,
        completion: &mut ring::Completion,
        cx: &mut Context,
    ) -> Option<bool> {
        // record if either of the rings were empty to start
        let was_empty = tx.is_empty() || completion.is_empty();

        // iterate twice to avoid waker registration races
        for i in 0..2 {
            let count = completion.acquire(u32::MAX);
            let count = tx.acquire(count).min(count);

            trace!("acquired {count} entries");

            // return if we have entries in both rings
            if count > 0 {
                return Some(was_empty);
            }

            // only wake the socket's driver if it's the first iteration
            if i == 0 {
                self.wake(tx, completion);
            }
        }

        // we need to keep polling until we have at least one item here
        if completion.is_empty() || tx.is_empty() {
            cx.waker().wake_by_ref();
        }

        Some(false)
    }

    #[inline]
    fn wake(&mut self, tx: &mut ring::Tx, _completion: &mut ring::Completion) {
        // wake up the driver if it's indicated we need to do so
        if tx.needs_wakeup() {
            let _ = syscall::wake_tx(tx.socket());
        }
    }
}

pub struct Channel<D: Driver> {
    pub tx: ring::Tx,
    pub completion: ring::Completion,
    pub driver: D,
}

impl<D: Driver> Channel<D> {
    #[inline]
    fn acquire(&mut self, cx: &mut Context) -> Option<bool> {
        // don't try to drive anything if the queues are both full
        if self.tx.is_full() && self.completion.is_full() {
            return Some(false);
        }

        trace!("acquiring channel capacity");

        self.driver.poll(&mut self.tx, &mut self.completion, cx)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.tx.is_empty() || self.completion.is_empty()
    }

    #[inline]
    fn wake(&mut self) {
        self.driver.wake(&mut self.tx, &mut self.completion);
    }
}

pub struct Tx<D: Driver> {
    channels: Vec<Channel<D>>,
    umem: Umem,
    encoder: encoder::State,
}

impl<D: Driver> Tx<D> {
    /// Creates a TX IO interface for an s2n-quic endpoint
    pub fn new(channels: Vec<Channel<D>>, umem: Umem, encoder: encoder::State) -> Self {
        Self {
            channels,
            umem,
            encoder,
        }
    }

    /// Consumes the TX endpoint into the inner channels
    ///
    /// This is used for internal tests only.
    #[cfg(test)]
    pub fn consume(self) -> Vec<Channel<D>> {
        self.channels
    }
}

impl<D: Driver> tx::Tx for Tx<D> {
    type PathHandle = path::Tuple;
    type Queue = Queue<'static, D>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // poll both channels to make sure we can make progress in both
        let mut is_any_ready = false;
        // assume all of the channels are closed until we get one that's not
        let mut is_all_closed = true;

        for (idx, channel) in self.channels.iter_mut().enumerate() {
            if let Some(did_become_ready) = channel.acquire(cx) {
                if did_become_ready {
                    trace!("channel {idx} became ready");
                }

                is_all_closed = false;
                is_any_ready |= did_become_ready;
            } else {
                trace!("channel {idx} closed");
            }
        }

        // if all of the channels are closed then shut down the task
        if is_all_closed {
            return Err(()).into();
        }

        if is_any_ready {
            trace!("tx ready");
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        let this: &'static mut Self = unsafe {
            // Safety: As noted in the [transmute examples](https://doc.rust-lang.org/std/mem/fn.transmute.html#examples)
            // it can be used to temporarily extend the lifetime of a reference. In this case, we
            // don't want to use GATs until the MSRV is >=1.65.0, which means `Self::Queue` is not
            // allowed to take generic lifetimes.
            //
            // We are left with using a `'static` lifetime here and encapsulating it in a private
            // field. The `Self::Queue` struct is then borrowed for the lifetime of the `F`
            // function. This will prevent the value from escaping beyond the lifetime of `&mut
            // self`.
            //
            // See https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=9a32abe85c666f36fb2ec86496cc41b4
            //
            // Once https://github.com/aws/s2n-quic/issues/1742 is resolved this code can go away
            core::mem::transmute(self)
        };

        let mut capacity = 0;

        let mut first_channel_with_entries = None;

        for (idx, channel) in this.channels.iter_mut().enumerate() {
            // make one more effort to acquire entries in the rings
            let len = channel.tx.acquire(1);
            let len = channel.completion.acquire(len).min(len);
            trace!("acquired {len} entries for channel {idx}");
            capacity += len as usize;

            if len > 0 && first_channel_with_entries.is_none() {
                first_channel_with_entries = Some(idx);
            }
        }

        let channels = &mut this.channels;
        let umem = &mut this.umem;
        let encoder = &mut this.encoder;

        // use the first channel that had entries, otherwise return the length, which will indicate
        // the queue has no free items
        let channel_index = first_channel_with_entries.unwrap_or(channels.len());

        let mut queue = Queue {
            channels,
            channel_index,
            channel_needs_wake: false,
            capacity,
            umem,
            encoder,
        };
        f(&mut queue);
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, _error: Self::Error, _events: &mut E) {
        // The only reason we would be returning an error is if a channel closed. This could either
        // be because the endpoint is shutting down or one of the tasks panicked. Either way, we
        // don't know what the cause is here so we don't have any events to emit.
    }
}

pub struct Queue<'a, D: Driver> {
    channels: &'a mut Vec<Channel<D>>,
    /// The current index into the channels list
    channel_index: usize,
    /// Indicates if the current channel needs to be woken up
    channel_needs_wake: bool,
    /// The remaining capacity of the queue
    capacity: usize,
    umem: &'a mut Umem,
    encoder: &'a mut encoder::State,
}

impl<D: Driver> tx::Queue for Queue<'_, D> {
    type Handle = path::Tuple;

    const SUPPORTS_ECN: bool = true;
    const SUPPORTS_FLOW_LABELS: bool = true;

    #[inline]
    fn push<M>(&mut self, mut message: M) -> Result<tx::Outcome, tx::Error>
    where
        M: tx::Message<Handle = Self::Handle>,
    {
        // if we're at capacity, then return an error
        if self.capacity == 0 {
            trace!("at capacity");
            return Err(tx::Error::AtCapacity);
        }

        let channel = loop {
            let channel = if let Some(channel) = self.channels.get_mut(self.channel_index) {
                channel
            } else {
                // we got to the end of the list without any more capacity
                return Err(tx::Error::AtCapacity);
            };

            // if this channel has entries, return it
            if !channel.is_empty() {
                trace!("selecting channel {}", self.channel_index);
                break channel;
            }

            // before moving on to the next channel, wake the current one if needed
            if core::mem::take(&mut self.channel_needs_wake) {
                trace!("waking channel {}", self.channel_index);
                channel.wake();
            }
            self.channel_index += 1;
        };

        // get the first descriptor in the ring
        let (entries, _) = channel.completion.data();
        let descriptor = entries[0];

        trace!("using descriptor {descriptor:?}");

        let buffer = unsafe {
            // Safety: this descriptor should be unique, assuming the tasks are functioning
            // properly
            self.umem.get_mut(descriptor)
        };

        // create an encoder for the descriptor region
        let mut buffer = EncoderBuffer::new(buffer);

        // write the message to the encoder using the configuration
        let payload_len = encoder::encode_packet(&mut buffer, &mut message, self.encoder)?;

        // take the length that we wrote and create a RxTxDescriptor with it
        let len = buffer.len();
        let descriptor = descriptor.with_len(len as _);

        trace!("packet written to {descriptor:?}");

        // push the descriptor on so it can be transmitted
        channel.tx.data().0[0] = descriptor;

        // make sure we give capacity back to the free queue
        channel.tx.release(1);
        channel.completion.release(1);

        // remember that we pushed something to the channel so it needs to be woken up
        self.channel_needs_wake = true;

        // decrement the total capacity after pushing another packet
        self.capacity -= 1;

        // let the caller know how big the payload was
        let outcome = tx::Outcome {
            len: payload_len as _,
            index: 0,
        };

        Ok(outcome)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<D: Driver> Drop for Queue<'_, D> {
    #[inline]
    fn drop(&mut self) {
        // if the current channel pushed some items it needs to be woken up
        if self.channel_needs_wake {
            if let Some(channel) = self.channels.get_mut(self.channel_index) {
                trace!("waking channel {}", self.channel_index);
                channel.wake();
            }
        }
    }
}
