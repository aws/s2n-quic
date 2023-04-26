// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{if_xdp::UmemDescriptor, ring};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::{
    slice::vectored_copy,
    sync::{spsc, worker},
};

type Receiver = spsc::Receiver<UmemDescriptor>;

/// Takes descriptors from RX workers and forwards it on to the fill queue
pub async fn rx_to_fill<N: Notifier>(mut rx_queues: Vec<Receiver>, fill: ring::Fill, notify: N) {
    match rx_queues.len() {
        0 => panic!("invalid rx queues"),
        1 => {
            trace!("using single queue mode");
            RxToFillRing {
                rxs: rx_queues.pop().unwrap(),
                fill,
                notify,
            }
            .await;
        }
        _ => {
            trace!("using multi-queue mode with {} queues", rx_queues.len());
            RxToFillRing {
                rxs: rx_queues,
                fill,
                notify,
            }
            .await;
        }
    }
}

/// Notifies the implementor of emitted packets on the fill queue
pub trait Notifier: Unpin {
    fn notify(&mut self, sent: u32, fill: &mut ring::Fill);
}

impl Notifier for () {
    #[inline]
    fn notify(&mut self, _send: u32, _fill: &mut ring::Fill) {
        // Nothing is usually needed here. The OS will pick up available entries on RX.
    }
}

impl Notifier for worker::Sender {
    #[inline]
    fn notify(&mut self, send: u32, _fill: &mut ring::Fill) {
        self.submit(send as _);
    }
}

/// A group of RX queues that are responsible for processing packets
trait Rxs: Unpin {
    /// Iterates over all of the queues in the group
    fn for_each<F: FnMut(&mut Receiver)>(&mut self, f: F);
    /// Returns the number of queues
    fn len(&self) -> usize;
}

impl Rxs for Vec<Receiver> {
    #[inline]
    fn for_each<F: FnMut(&mut Receiver)>(&mut self, mut f: F) {
        for s in self.iter_mut() {
            f(s);
        }
    }

    #[inline]
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl Rxs for Receiver {
    #[inline]
    fn for_each<F: FnMut(&mut Receiver)>(&mut self, mut f: F) {
        f(self);
    }

    #[inline]
    fn len(&self) -> usize {
        1
    }
}

struct RxToFillRing<R: Rxs, N: Notifier> {
    rxs: R,
    fill: ring::Fill,
    notify: N,
}

impl<R: Rxs, N: Notifier> Future for RxToFillRing<R, N> {
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let Self { rxs, fill, notify } = self.get_mut();

        trace!("polling rx to fill ring");

        let mut sent = 0;
        let mut closed = 0;

        let mut has_fill_capacity = true;

        rxs.for_each(|rx| {
            // we need to loop until we can't read anything so the waker stays registered
            while has_fill_capacity {
                trace!("polling rx queue");

                match rx.poll_slice(cx) {
                    Poll::Ready(Ok(mut slice)) => {
                        let (from_a, from_b) = slice.peek();
                        let expected_len = (from_a.len() + from_b.len()) as u32;
                        debug_assert_ne!(expected_len, 0);

                        trace!("rx queue has {} items available", expected_len);

                        // acquire entries to submit to the fill queue
                        let actual_len = fill.acquire(expected_len);

                        trace!("acquired {actual_len} items from the Fill queue");

                        let (to_a, to_b) = fill.data();

                        // copy all of the items from the worker's queue into the fill queue
                        let copied_len = vectored_copy(&[from_a, from_b], &mut [to_a, to_b]);

                        trace!("moved {copied_len} items into the Fill queue");

                        // release all of the entries we copied
                        slice.release(copied_len);
                        fill.release(copied_len as _);

                        sent += copied_len as u32;

                        // the fill queue didn't have enough capacity for us to fill. make a last
                        // effort to acquire capacity or try again later.
                        if expected_len > actual_len {
                            if fill.acquire(u32::MAX) > 0 {
                                // we got something; keep filling it
                                continue;
                            }

                            // we didn't get anything; yield and wake up immediately
                            cx.waker().wake_by_ref();
                            has_fill_capacity = false;
                            break;
                        }
                    }
                    Poll::Ready(Err(_)) => {
                        trace!("rx queue closed");
                        closed += 1;
                        break;
                    }
                    Poll::Pending => {
                        // we cleared the queue and registered our waker so go to the next queue
                        trace!("rx queue empty");
                        break;
                    }
                }
            }
        });

        // submit the number of items that we sent to the fill queue
        trace!("notifying that {sent} items were submitted to the fill queue");
        notify.notify(sent, fill);

        // if all of the queues are closed then shut down the task
        if closed == rxs.len() {
            trace!("all RX senders are closed; shutting down");
            return Poll::Ready(());
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        if_xdp::UmemDescriptor,
        task::testing::{random_delay, QUEUE_SIZE, TEST_ITEMS},
    };
    use rand::prelude::*;
    use tokio::sync::oneshot;

    async fn execute_test(workers: usize) {
        let channel_size = QUEUE_SIZE;
        let worker_total = TEST_ITEMS / workers;
        let expected_total = worker_total * workers;

        let mut worker_channels = vec![];
        for idx in 0..workers {
            let (mut tx_send, tx_recv) = spsc::channel(channel_size);
            worker_channels.push(tx_recv);

            tokio::spawn(async move {
                let mut addresses = (idx as u64..)
                    .step_by(workers)
                    .take(worker_total)
                    .map(|address| UmemDescriptor { address })
                    .peekable();

                while addresses.peek().is_some() {
                    if tx_send.acquire().await.is_err() {
                        trace!("TX receiver closed; shutting down");
                        return;
                    }

                    let mut slice = tx_send.slice();

                    let batch_size = thread_rng().gen_range(1..=slice.capacity());

                    trace!("TX batch size set to {batch_size}");

                    for desc in (&mut addresses).take(batch_size) {
                        trace!("sending address {}", desc.address);
                        let _ = slice.push(desc);
                    }

                    random_delay().await;
                }

                trace!("all items sent; shutting down");
            });
        }

        let (mut ring_rx, ring_tx) = ring::testing::completion_fill(channel_size as u32);
        let (worker_send, mut worker_recv) = worker::channel();
        let (done_send, done_recv) = oneshot::channel();

        tokio::spawn(rx_to_fill(worker_channels, ring_tx, worker_send));

        tokio::spawn(async move {
            let mut totals: Vec<_> = (0..workers as u64).collect();
            let mut total = 0;

            while let Some(credits) = worker_recv.acquire().await {
                trace!("acquired {credits} worker credits");

                let count = ring_rx.acquire(1);

                trace!("acquired {count} RX ring entries");

                let count = credits.min(count as _);

                if count == 0 {
                    continue;
                }

                let (head, tail) = ring_rx.data();
                for entry in head.iter().chain(tail.iter()).take(count) {
                    trace!("receiving address {}", entry.address);

                    let worker = entry.address as usize % workers;
                    let worker_total = &mut totals[worker];
                    assert_eq!(*worker_total, entry.address);
                    *worker_total += workers as u64;
                }

                trace!("received {count} items");

                ring_rx.release(count as _);
                worker_recv.finish(count as _);
                total += count as u64;
            }

            trace!("receiver finished; shutting down");

            done_send.send(total).unwrap();
        });

        let actual_total = done_recv.await.unwrap();

        assert_eq!(expected_total as u64, actual_total);
    }

    #[tokio::test]
    async fn single_worker() {
        execute_test(1).await;
    }

    #[tokio::test]
    async fn multiple_worker() {
        execute_test(4).await;
    }
}
