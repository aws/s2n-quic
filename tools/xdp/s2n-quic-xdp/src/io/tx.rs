// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{RxTxDescriptor, UmemDescriptor},
    umem::Umem,
};
use core::{
    cell::UnsafeCell,
    task::{Context, Poll},
};
use s2n_codec::{Encoder as _, EncoderBuffer};
use s2n_quic_core::{
    event,
    io::tx,
    sync::spsc,
    xdp::{encoder, path},
};

pub type Free = spsc::Receiver<UmemDescriptor>;
pub type Occupied = spsc::Sender<RxTxDescriptor>;

pub struct Tx {
    channels: UnsafeCell<Vec<(Free, Occupied)>>,
    /// Store a vec of slices on the struct so we don't have to allocate every time `queue` is
    /// called. Since this causes the type to be self-referential it does need a bit of unsafe code
    /// to pull this off.
    slices: UnsafeCell<
        Vec<(
            spsc::RecvSlice<'static, UmemDescriptor>,
            spsc::SendSlice<'static, RxTxDescriptor>,
        )>,
    >,
    umem: Umem,
    encoder: encoder::State,
    is_full: bool,
}

impl Tx {
    /// Creates a TX IO interface for an s2n-quic endpoint
    pub fn new(channels: Vec<(Free, Occupied)>, umem: Umem, encoder: encoder::State) -> Self {
        let slices = UnsafeCell::new(Vec::with_capacity(channels.len()));
        let channels = UnsafeCell::new(channels);
        Self {
            channels,
            slices,
            umem,
            encoder,
            is_full: true,
        }
    }

    /// Consumes the TX endpoint into the inner channels
    ///
    /// This is used for internal tests only.
    #[cfg(test)]
    pub fn consume(self) -> Vec<(Free, Occupied)> {
        self.channels.into_inner()
    }
}

impl tx::Tx for Tx {
    type PathHandle = path::Tuple;
    type Queue = Queue<'static>;
    type Error = spsc::ClosedError;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // If we didn't fill up the queue then we don't need to poll for capacity
        if !self.is_full {
            return Poll::Pending;
        }

        // poll both channels to make sure we can make progress in both
        let mut is_any_ready = false;
        let mut is_all_free_closed = true;
        let mut is_all_occupied_closed = true;

        for (free, occupied) in self.channels.get_mut() {
            let mut is_ready = true;

            macro_rules! ready {
                ($slice:ident, $closed:ident) => {
                    match $slice.poll_slice(cx) {
                        Poll::Ready(Ok(_)) => {
                            $closed = false;
                        }
                        Poll::Ready(Err(_)) => {
                            // defer returning an error until all slices return one
                        }
                        Poll::Pending => {
                            $closed = false;
                            is_ready = false
                        }
                    }
                };
            }

            ready!(occupied, is_all_occupied_closed);
            ready!(free, is_all_free_closed);

            is_any_ready |= is_ready;
        }

        if is_all_free_closed || is_all_occupied_closed {
            return Err(spsc::ClosedError).into();
        }

        if is_any_ready {
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

        let slices = this.slices.get_mut();

        let mut capacity = 0;

        for (free, occupied) in this.channels.get_mut().iter_mut() {
            let mut free = free.slice();
            let mut occupied = occupied.slice();

            // try to synchronize the peer's queues
            let _ = free.sync();
            let _ = occupied.sync();

            if free.is_empty() || occupied.capacity() == 0 {
                continue;
            }

            capacity += free.len().min(occupied.capacity());
            slices.push((free, occupied));
        }

        // update our full status
        this.is_full = slices.is_empty();

        let umem = &mut this.umem;
        let encoder = &mut this.encoder;
        let is_full = &mut this.is_full;

        let mut queue = Queue {
            slices,
            slice_index: 0,
            capacity,
            umem,
            encoder,
            is_full,
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

pub struct Queue<'a> {
    slices: &'a mut Vec<(
        spsc::RecvSlice<'a, UmemDescriptor>,
        spsc::SendSlice<'a, RxTxDescriptor>,
    )>,
    slice_index: usize,
    capacity: usize,
    umem: &'a mut Umem,
    encoder: &'a mut encoder::State,
    is_full: &'a mut bool,
}

impl<'a> tx::Queue for Queue<'a> {
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
            return Err(tx::Error::AtCapacity);
        }

        let (free, occupied) = unsafe {
            // Safety: the slice index should always be in bounds
            self.slices.get_unchecked_mut(self.slice_index)
        };

        // take the first free descriptor, we should have at least one item
        let (head, _) = free.peek();
        let descriptor = head[0];

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

        // push the descriptor on so it can be transmitted
        let result = occupied.push(descriptor);

        debug_assert!(
            result.is_ok(),
            "occupied queue should have capacity {result:?}"
        );

        // make sure we give capacity back to the free queue
        free.release(1);

        // if this slice is at capacity then increment the index and try the next one
        if free.is_empty() || occupied.capacity() == 0 {
            self.slice_index += 1;
        }

        // check to see if we're full now
        self.capacity -= 1;
        *self.is_full = self.capacity == 0;

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

impl<'a> Drop for Queue<'a> {
    #[inline]
    fn drop(&mut self) {
        // make sure we drop all of the slices to flush our changes
        self.slices.clear();
    }
}
