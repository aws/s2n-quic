// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Rx;
use crate::{event, inet::datagram};
use core::task::{Context, Poll};

/// A pair of Rx channels that feed into the same endpoint
pub struct Channel<A, B>
where
    A: Rx,
    B: Rx<PathHandle = A::PathHandle>,
{
    pub(super) a: A,
    pub(super) b: B,
}

impl<A, B> Rx for Channel<A, B>
where
    A: Rx,
    B: Rx<PathHandle = A::PathHandle>,
    A::Queue: 'static,
    B::Queue: 'static,
{
    type PathHandle = A::PathHandle;
    type Queue = Queue<'static, A::Queue, B::Queue>;
    type Error = Error<A::Error, B::Error>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // assume we aren't ready until one channel returns ready
        let mut is_ready = false;

        macro_rules! ready {
            ($value:expr, $var:ident) => {
                match $value {
                    Poll::Ready(Ok(())) => is_ready = true,
                    Poll::Ready(Err(err)) => {
                        // one of the channels returned an error so shut down both
                        return Err(Error::$var(err)).into();
                    }
                    Poll::Pending => {}
                }
            };
        }

        ready!(self.a.poll_ready(cx), A);
        ready!(self.b.poll_ready(cx), B);

        if is_ready {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        let a = &mut self.a;
        let b = &mut self.b;
        a.queue(|a| {
            b.queue(|b| {
                let (a, b): (&'static mut _, &'static mut _) = unsafe {
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
                    (core::mem::transmute(a), core::mem::transmute(b))
                };

                let mut queue = Queue { a, b };
                f(&mut queue);
            });
        });
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, error: Self::Error, event: &mut E) {
        // dispatch the error to the appropriate channel
        match error {
            Error::A(error) => self.a.handle_error(error, event),
            Error::B(error) => self.b.handle_error(error, event),
        }
    }
}

/// Tagged error for a pair of channels
pub enum Error<A, B> {
    A(A),
    B(B),
}

pub struct Queue<'a, A, B>
where
    A: super::Queue,
    B: super::Queue,
{
    a: &'a mut A,
    b: &'a mut B,
}

impl<'a, A, B> super::Queue for Queue<'a, A, B>
where
    A: super::Queue,
    B: super::Queue<Handle = A::Handle>,
{
    type Handle = A::Handle;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        // drain both of the channels
        self.a.for_each(&mut on_packet);
        self.b.for_each(&mut on_packet);
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.a.is_empty() && self.b.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{
        rx::{Queue as _, RxExt as _},
        testing,
    };
    use futures_test::task::noop_waker;

    #[test]
    fn pair_test() {
        let channel_a = testing::Channel::default();
        let channel_b = testing::Channel::default();
        let mut merged = channel_a.clone().with_pair(channel_b.clone());

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let cx = &mut cx;

        for push_a in [false, true] {
            for push_b in [false, true] {
                assert!(merged.poll_ready(cx).is_pending());

                let mut expected = 0;

                if push_a {
                    expected += 1;
                    channel_a.push(Default::default());
                }

                if push_b {
                    expected += 1;
                    channel_b.push(Default::default());
                }

                assert_eq!(merged.poll_ready(cx).is_ready(), push_a || push_b);

                let mut actual = 0;
                merged.queue(|queue| {
                    assert_eq!(queue.is_empty(), !(push_a || push_b));

                    queue.for_each(|_header, _payload| {
                        actual += 1;
                    });
                });

                assert_eq!(expected, actual);
            }
        }
    }
}
