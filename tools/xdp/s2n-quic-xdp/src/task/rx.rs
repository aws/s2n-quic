// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{if_xdp::RxTxDescriptor, ring, socket, syscall};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::sync::{spsc, worker};

/// Polls a RX queue for entries and sends them to the notifier
pub async fn rx<P: Poller, N: Notifier>(poller: P, rx: ring::Rx, notifier: N) {
    Rx {
        poller,
        rx,
        notifier,
    }
    .await;
}

#[cfg(feature = "tokio")]
mod tokio_impl;

/// Polls a socket for pending RX items
pub trait Poller: Unpin {
    fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
        &mut self,
        rx: &mut ring::Rx,
        cx: &mut Context,
        on_ready: F,
    ) -> Poll<Result<(), ()>>;
}

/// Busy polls a socket
impl Poller for socket::Fd {
    #[inline]
    fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
        &mut self,
        rx: &mut ring::Rx,
        cx: &mut Context,
        mut on_ready: F,
    ) -> Poll<Result<(), ()>> {
        let _ = syscall::busy_poll(self);

        // wake up the task immediately after
        cx.waker().wake_by_ref();

        // try to acquire entries from the RX queue
        let count = rx.acquire(1);

        // we didn't get anything; try again later
        if count == 0 {
            return Poll::Pending;
        }

        // notify the callback that we have some items
        if on_ready(rx, cx).is_none() {
            return Poll::Ready(Err(()));
        }

        Poll::Ready(Ok(()))
    }
}

/// Polling implementation using a worker Receiver
///
/// This is mostly used in testing. Real-world applications will likely use an actual socket.
impl Poller for worker::Receiver {
    #[inline]
    fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
        &mut self,
        rx: &mut ring::Rx,
        cx: &mut Context,
        mut on_ready: F,
    ) -> Poll<Result<(), ()>> {
        // limit the number of loops to prevent endless spinning on registering wakers
        for iteration in 0..10 {
            trace!("iteration {}", iteration);

            // try to acquire work items
            match self.poll_acquire(cx) {
                Poll::Ready(Some(items)) => {
                    trace!("acquired {items} items from worker");

                    // try to acquire entries for the queue
                    let count = rx.acquire(items as _) as usize;

                    trace!("acquired {count} items from RX ring");

                    // if we didn't get anything, try to acquire RX entries again
                    if count == 0 {
                        continue;
                    }

                    // we have at least one entry so notify the callback
                    match on_ready(rx, cx) {
                        Some(actual) => {
                            trace!("consumed {actual} items");

                            self.finish(actual);

                            continue;
                        }
                        None => {
                            trace!("on_ready closed; closing receiver");

                            return Poll::Ready(Err(()));
                        }
                    }
                }
                Poll::Ready(None) => {
                    trace!("worker sender closed; closing poller");

                    return Poll::Ready(Err(()));
                }
                Poll::Pending => {
                    trace!("worker out of items; sleeping");

                    return Poll::Pending;
                }
            }
        }

        // if we got here, we iterated 10 times and need to yield so we don't consume the event
        // loop too much
        trace!("waking self");
        cx.waker().wake_by_ref();

        Poll::Pending
    }
}

/// Notifies an RX worker than new entries are available
pub trait Notifier: Unpin {
    fn notify(
        &mut self,
        head: &mut [RxTxDescriptor],
        tail: &mut [RxTxDescriptor],
        cx: &mut Context,
    ) -> Option<usize>;
}

impl Notifier for spsc::Sender<RxTxDescriptor> {
    #[inline]
    fn notify(
        &mut self,
        head: &mut [RxTxDescriptor],
        tail: &mut [RxTxDescriptor],
        cx: &mut Context,
    ) -> Option<usize> {
        trace!(
            "notifying rx queue of {} available items",
            head.len() + tail.len()
        );

        match self.poll_slice(cx) {
            Poll::Ready(Ok(mut slice)) => {
                trace!("rx queue has capacity of {}", slice.capacity());

                let mut pushed = 0;

                /// copies the provided entries into the RX queue
                macro_rules! extend {
                    ($name:ident) => {
                        if !$name.is_empty() {
                            let mut iter = $name
                                .iter()
                                .map(|v| {
                                    pushed += 1;
                                    *v
                                })
                                .peekable();

                            while iter.peek().is_some() {
                                if slice.extend(&mut iter).is_err() {
                                    trace!("rx queue closed; closing");
                                    return None;
                                }
                            }
                        }
                    };
                }

                extend!(head);
                extend!(tail);

                trace!("rx queue pushed {pushed} items");

                Some(pushed)
            }
            Poll::Ready(Err(_)) => {
                trace!("rx queue closed; closing");
                None
            }
            Poll::Pending => {
                trace!("no rx capacity available; sleeping");
                Some(0)
            }
        }
    }
}

struct Rx<P: Poller, N: Notifier> {
    poller: P,
    rx: ring::Rx,
    notifier: N,
}

impl<P: Poller, N: Notifier> Future for Rx<P, N> {
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let Self {
            poller,
            rx,
            notifier,
        } = self.get_mut();

        trace!("polling rx");

        match poller.poll(rx, cx, |rx, cx| {
            let (head, tail) = rx.data();
            let len = head.len() + tail.len();

            let actual = notifier.notify(head, tail, cx)?;

            debug_assert!(
                actual <= len,
                "the number of actual items should not exceed what was acquired"
            );

            // While we have a `debug_assert` above, this is being overly defensive just in case.
            // In regular conditions, it's equivalent to just releasing `actual`.
            let len = len.min(actual);

            // release the entries back to the RX ring
            rx.release(len as _);

            Some(len)
        }) {
            Poll::Ready(Ok(())) => Poll::Pending,
            Poll::Ready(Err(_)) => Poll::Ready(()),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        if_xdp::UmemDescriptor,
        task::testing::{random_delay, QUEUE_SIZE_LARGE, QUEUE_SIZE_SMALL, TEST_ITEMS},
    };
    use rand::prelude::*;
    use tokio::sync::oneshot;

    async fn execute_test(channel_size: usize) {
        let expected_total = TEST_ITEMS as u64;

        let (rx_send, mut rx_recv) = spsc::channel(channel_size);
        let (ring_rx, mut ring_tx) = ring::testing::rx_tx(channel_size as u32);
        let (worker_send, worker_recv) = worker::channel();
        let (done_send, done_recv) = oneshot::channel();

        tokio::spawn(rx(worker_recv, ring_rx, rx_send));

        tokio::spawn(async move {
            let mut addresses = (0..expected_total)
                .map(|address| UmemDescriptor { address }.with_len(0))
                .peekable();

            let mut total = 0;

            while addresses.peek().is_some() {
                let count = ring_tx.acquire(1);

                if count == 0 {
                    trace!("no capacity in TX ring; sleeping");
                    random_delay().await;
                    continue;
                }

                let batch_size = thread_rng().gen_range(1..=count);
                let (head, tail) = ring_tx.data();

                trace!("submitting {batch_size} items to TX ring");

                let mut sent = 0;
                for (desc, dest) in (&mut addresses)
                    .take(batch_size as _)
                    .zip(head.iter_mut().chain(tail))
                {
                    trace!("send entry address: {}", desc.address);

                    *dest = desc;
                    sent += 1;
                }

                ring_tx.release(sent as _);
                worker_send.submit(sent as _);
                total += sent;

                random_delay().await;
            }

            assert_eq!(total, expected_total);
            trace!("sender shutting down");
        });

        tokio::spawn(async move {
            let mut total = 0;

            while rx_recv.acquire().await.is_ok() {
                let mut slice = rx_recv.slice();

                trace!("waking up receiver with {} items", slice.len());

                while let Some(desc) = slice.pop() {
                    trace!("recv entry address: {}", desc.address);

                    assert_eq!(
                        desc.address, total,
                        "address does not match the expected value"
                    );
                    total += 1;
                }
            }

            trace!("receiver shutting down");
            done_send.send(total).unwrap();
        });

        let actual_total = done_recv.await.unwrap();

        assert_eq!(expected_total, actual_total);
    }

    #[tokio::test]
    async fn rx_small_test() {
        execute_test(QUEUE_SIZE_SMALL).await;
    }

    #[tokio::test]
    async fn rx_large_test() {
        execute_test(QUEUE_SIZE_LARGE).await;
    }
}
