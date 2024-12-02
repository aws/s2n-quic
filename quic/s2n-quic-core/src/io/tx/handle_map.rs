// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, inet::ExplicitCongestionNotification, io::tx, path};
use core::{
    marker::PhantomData,
    task::{Context, Poll},
    time::Duration,
};

pub struct Channel<Map, Tx, U> {
    pub(super) map: Map,
    pub(super) tx: Tx,
    pub(super) handle: PhantomData<U>,
}

impl<Map, Tx, U> tx::Tx for Channel<Map, Tx, U>
where
    Map: 'static + Fn(&U) -> Tx::PathHandle,
    Tx: tx::Tx,
    Tx::Queue: 'static,
    U: path::Handle,
{
    type PathHandle = U;
    type Queue = Queue<'static, Map, Tx::Queue, U>;
    type Error = Tx::Error;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_ready(cx)
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        let map = &mut self.map;
        let tx = &mut self.tx;
        tx.queue(|tx| {
            let (map, tx): (&'static mut _, &'static mut _) = unsafe {
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
                (
                    core::mem::transmute::<&mut Map, &mut Map>(map),
                    core::mem::transmute::<&mut <Tx as tx::Tx>::Queue, &mut <Tx as tx::Tx>::Queue>(
                        tx,
                    ),
                )
            };

            let mut queue = Queue {
                map,
                tx,
                handle: PhantomData,
            };
            f(&mut queue);
        });
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, error: Self::Error, events: &mut E) {
        self.tx.handle_error(error, events)
    }
}

pub struct Queue<'a, Map, Tx, U>
where
    Map: Fn(&U) -> Tx::Handle,
    Tx: tx::Queue,
{
    map: &'a Map,
    tx: &'a mut Tx,
    handle: PhantomData<U>,
}

impl<Map, Tx, U> tx::Queue for Queue<'_, Map, Tx, U>
where
    Map: Fn(&U) -> Tx::Handle,
    Tx: tx::Queue,
    U: path::Handle,
{
    type Handle = U;

    const SUPPORTS_ECN: bool = Tx::SUPPORTS_ECN;
    const SUPPORTS_PACING: bool = Tx::SUPPORTS_PACING;
    const SUPPORTS_FLOW_LABELS: bool = Tx::SUPPORTS_FLOW_LABELS;

    #[inline]
    fn push<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        inner: M,
    ) -> Result<tx::Outcome, tx::Error> {
        let handle = (self.map)(inner.path_handle());
        let message = Message { inner, handle };
        self.tx.push(message)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    #[inline]
    fn has_capacity(&self) -> bool {
        self.tx.has_capacity()
    }
}

pub struct Message<M, Handle> {
    inner: M,
    handle: Handle,
}

impl<M, Handle> tx::Message for Message<M, Handle>
where
    M: tx::Message,
    Handle: path::Handle,
{
    type Handle = Handle;

    #[inline]
    fn path_handle(&self) -> &Self::Handle {
        // use the mapped handle instead of the inner type
        &self.handle
    }

    #[inline]
    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.inner.ecn()
    }

    #[inline]
    fn delay(&mut self) -> Duration {
        self.inner.delay()
    }

    #[inline]
    fn ipv6_flow_label(&mut self) -> u32 {
        self.inner.ipv6_flow_label()
    }

    #[inline]
    fn can_gso(&self, segment_len: usize, segment_count: usize) -> bool {
        self.inner.can_gso(segment_len, segment_count)
    }

    #[inline]
    fn write_payload(
        &mut self,
        buffer: tx::PayloadBuffer,
        gso_offset: usize,
    ) -> Result<usize, tx::Error> {
        self.inner.write_payload(buffer, gso_offset)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        io::{
            testing,
            tx::{Queue as _, Tx as _, TxExt as _},
        },
        path::{Handle as _, RemoteAddress},
    };

    #[test]
    fn handle_map_test() {
        let channel = testing::Channel::default();
        let mut mapped = channel.clone().with_handle_map(|handle: &RemoteAddress| {
            let mut handle = testing::Handle::from_remote_address(*handle);
            handle.local_address.set_port(321);
            handle
        });

        mapped.queue(|queue| {
            let mut handle = RemoteAddress::default();
            handle.set_port(123);
            let msg = (handle, &[1, 2, 3][..]);
            queue.push(msg).unwrap();
        });

        let msg = channel.pop().unwrap();

        assert_eq!(msg.header.path.remote_address.port(), 123);
        assert_eq!(msg.header.path.local_address.port(), 321);
    }
}
