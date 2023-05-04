// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{RxTxDescriptor, UmemDescriptor},
    umem::Umem,
};
use core::task::{Context, Poll};
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
    free: Free,
    occupied: Occupied,
    umem: Umem,
    encoder: encoder::State,
    is_full: bool,
}

impl Tx {
    /// Creates a TX IO interface for an s2n-quic endpoint
    pub fn new(free: Free, occupied: Occupied, umem: Umem, encoder: encoder::State) -> Self {
        Self {
            occupied,
            free,
            umem,
            encoder,
            is_full: false,
        }
    }

    /// Consumes the TX endpoint into the inner channels
    ///
    /// This is used for internal tests only.
    #[cfg(test)]
    pub fn consume(self) -> (Free, Occupied) {
        (self.free, self.occupied)
    }
}

impl tx::Tx for Tx {
    type PathHandle = path::Tuple;
    type Queue = Queue<'static>;
    type Error = spsc::ClosedError;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // poll both channels to make sure we can make progress in both
        let free = self.free.poll_slice(cx);
        let occupied = self.occupied.poll_slice(cx);

        ready!(free)?;
        ready!(occupied)?;

        // we only need to wake up if the queue was previously completely filled up
        if !self.is_full {
            return Poll::Pending;
        }

        Poll::Ready(Ok(()))
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

        let mut free = this.free.slice();
        let mut occupied = this.occupied.slice();

        // if we were full, then try to synchronize the peer's queues
        if this.is_full {
            let _ = free.sync();
            let _ = occupied.sync();
        }

        // update our full status
        this.is_full = free.is_empty() || occupied.capacity() == 0;

        let umem = &mut this.umem;
        let encoder = &mut this.encoder;
        let is_full = &mut this.is_full;

        let mut queue = Queue {
            free,
            occupied,
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
    free: spsc::RecvSlice<'a, UmemDescriptor>,
    occupied: spsc::SendSlice<'a, RxTxDescriptor>,
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
        if *self.is_full {
            return Err(tx::Error::AtCapacity);
        }

        // take the first free descriptor, we should have at least one item
        let (head, _) = self.free.peek();
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
        let result = self.occupied.push(descriptor);

        debug_assert!(
            result.is_ok(),
            "occupied queue should have capacity {result:?}"
        );

        // make sure we give capacity back to the free queue
        self.free.release(1);

        // check to see if we're full now
        *self.is_full = !self.has_capacity();

        // let the caller know how big the payload was
        let outcome = tx::Outcome {
            len: payload_len as _,
            index: 0,
        };

        Ok(outcome)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.free.len().min(self.occupied.capacity())
    }
}
