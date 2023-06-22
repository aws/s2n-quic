// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    inet::datagram,
    io::{rx, tx},
    path::Tuple,
};
use core::task::{Context, Poll};
use std::sync::{Arc, Mutex};

pub type Handle = Tuple;

#[derive(Clone, Debug, Default)]
pub struct Channel {
    messages: Arc<Mutex<Vec<Message>>>,
}

impl Channel {
    pub fn push(&self, message: Message) {
        self.messages.lock().unwrap().push(message);
    }

    pub fn pop(&self) -> Option<Message> {
        self.messages.lock().unwrap().pop()
    }

    #[inline]
    fn queue<F: FnOnce(&mut Queue<'static>)>(&mut self, f: F) {
        if let Ok(mut messages) = self.messages.lock() {
            let messages: &mut Vec<_> = &mut *messages;
            let messages: &'static mut _ = unsafe {
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
                core::mem::transmute(messages)
            };

            let mut queue = Queue { messages };
            f(&mut queue);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub header: datagram::Header<Tuple>,
    pub payload: Vec<u8>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            header: datagram::Header {
                ecn: Default::default(),
                path: Tuple {
                    local_address: Default::default(),
                    remote_address: Default::default(),
                },
            },
            payload: Default::default(),
        }
    }
}

impl tx::Tx for Channel {
    type PathHandle = Tuple;
    type Queue = Queue<'static>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // the TX channel is always ready
        Poll::Pending
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        Self::queue(self, f)
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, _error: Self::Error, _event: &mut E) {
        // nothing to do
    }
}

impl rx::Rx for Channel {
    type PathHandle = Tuple;
    type Queue = Queue<'static>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let messages = self.messages.lock().map_err(|_| ())?;
        if messages.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        Self::queue(self, f)
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, _error: Self::Error, _event: &mut E) {
        // nothing to do
    }
}

pub struct Queue<'a> {
    messages: &'a mut Vec<Message>,
}

impl<'a> tx::Queue for Queue<'a> {
    type Handle = Tuple;

    #[inline]
    fn push<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<tx::Outcome, tx::Error> {
        let mut out = Message::default();
        out.header.ecn = message.ecn();
        out.header.path = *message.path_handle();

        out.payload.resize(1500, 0);
        let buffer = tx::PayloadBuffer::new(&mut out.payload);
        let len = message.write_payload(buffer, 0)?;

        self.messages.push(out);

        let outcome = tx::Outcome { index: 0, len };

        Ok(outcome)
    }

    #[inline]
    fn capacity(&self) -> usize {
        usize::MAX - self.messages.len()
    }
}

impl<'a> rx::Queue for Queue<'a> {
    type Handle = Tuple;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        for mut message in self.messages.drain(..) {
            on_packet(message.header, &mut message.payload);
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}
