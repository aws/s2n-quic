// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{if_xdp::RxTxDescriptor, ring, socket, syscall};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::{
    slice::vectored_copy,
    sync::{spsc, worker},
};

/// Takes a queue of descriptors to be transmitted on a socket
pub async fn tx<N: Notifier>(
    outgoing: spsc::Receiver<RxTxDescriptor>,
    tx: ring::Tx,
    notifier: N,
    worker: worker::Sender,
) {
    Tx {
        outgoing,
        tx,
        notifier,
        worker,
    }
    .await;
}

/// Notifies the implementor of progress on the TX ring
pub trait Notifier: Unpin {
    fn notify(&mut self);
}

impl Notifier for () {
    #[inline]
    fn notify(&mut self) {
        // nothing to do
    }
}

impl Notifier for socket::Fd {
    #[inline]
    fn notify(&mut self) {
        let result = syscall::wake_tx(self);

        trace!("waking tx for progress {result:?}");
    }
}

struct Tx<N: Notifier> {
    outgoing: spsc::Receiver<RxTxDescriptor>,
    tx: ring::Tx,
    notifier: N,
    worker: worker::Sender,
}

impl<N: Notifier> Future for Tx<N> {
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let Self {
            outgoing,
            tx,
            notifier,
            worker,
        } = self.get_mut();

        trace!("polling tx");

        for iteration in 0..10 {
            trace!("iteration {}", iteration);

            let count = match outgoing.poll_slice(cx) {
                Poll::Ready(Ok(slice)) => slice.len() as u32,
                Poll::Ready(Err(_)) => {
                    trace!("tx queue is closed; shutting down");
                    return Poll::Ready(());
                }
                Poll::Pending => {
                    trace!("tx queue out of items; sleeping");
                    return Poll::Pending;
                }
            };

            trace!("acquired {count} items from tx queues");

            let count = tx.acquire(count);

            trace!("acquired {count} items from TX ring");

            if count == 0 {
                notifier.notify();
                continue;
            }

            let mut outgoing = outgoing.slice();
            let (rx_head, rx_tail) = outgoing.peek();
            let (tx_head, tx_tail) = tx.data();

            let count = vectored_copy(&[rx_head, rx_tail], &mut [tx_head, tx_tail]);

            trace!("copied {count} items into TX ring");

            if count > 0 {
                tx.release(count as _);
                outgoing.release(count);
                worker.submit(count);
            }

            if tx.needs_wakeup() {
                trace!("TX ring needs wakeup");
                notifier.notify();
            }
        }

        // if we got here, we iterated 10 times and need to yield so we don't consume the event
        // loop too much
        trace!("waking self");
        cx.waker().wake_by_ref();
        Poll::Pending
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

        let (mut tx_send, tx_recv) = spsc::channel(channel_size);
        let (mut ring_rx, ring_tx) = ring::testing::rx_tx(channel_size as u32);
        let (worker_send, mut worker_recv) = worker::channel();
        let (done_send, done_recv) = oneshot::channel();

        tokio::spawn(tx(tx_recv, ring_tx, (), worker_send));

        tokio::spawn(async move {
            let mut addresses = (0..expected_total)
                .map(|address| UmemDescriptor { address }.with_len(0))
                .peekable();

            while addresses.peek().is_some() {
                if tx_send.acquire().await.is_err() {
                    return;
                }

                let batch_size = thread_rng().gen_range(1..channel_size);
                let mut slice = tx_send.slice();

                let _ = slice.extend(&mut (&mut addresses).take(batch_size));

                random_delay().await;
            }
        });

        tokio::spawn(async move {
            let mut total = 0;

            while let Some(credits) = worker_recv.acquire().await {
                let actual = ring_rx.acquire(1);

                if actual == 0 {
                    continue;
                }

                let (head, tail) = ring_rx.data();
                for entry in head.iter().chain(tail.iter()) {
                    assert_eq!(entry.address, total);
                    total += 1;
                }

                ring_rx.release(actual);
                worker_recv.finish(credits);
            }

            done_send.send(total).unwrap();
        });

        let actual_total = done_recv.await.unwrap();

        assert_eq!(expected_total, actual_total);
    }

    #[tokio::test]
    async fn tx_small_test() {
        execute_test(QUEUE_SIZE_SMALL).await;
    }

    #[tokio::test]
    async fn tx_large_test() {
        execute_test(QUEUE_SIZE_LARGE).await;
    }
}
