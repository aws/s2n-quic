// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{if_xdp::UmemDescriptor, ring};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::sync::{spsc, worker};

mod assign;

type Sender = spsc::Sender<UmemDescriptor>;

/// Takes descriptors from the completion queue and forwards it to individual workers
pub async fn completion_to_tx<P: Poller>(
    poller: P,
    comp: ring::Completion,
    frame_size: u32,
    mut tx_queues: Vec<Sender>,
) {
    for tx_queue in &tx_queues {
        assert!(
            tx_queue.capacity() >= comp.capacity(),
            "tx queues should have at least as much capacity as the completion queue to avoid dropping descriptors"
        );
    }

    // create a different future based on the arguments
    match (
        tx_queues.len(),
        frame_size.is_power_of_two(),
        tx_queues.len().is_power_of_two(),
    ) {
        (0, _, _) => panic!("invalid tx_queues size"),
        (1, _, _) => {
            trace!("using single queue mode");
            CompletionRingToTx {
                txs: tx_queues.pop().unwrap(),
                comp,
                poller,
                assignment: (),
            }
            .await;
        }
        (len, true, true) => {
            trace!("using fully-aligned mode with {len} queues");
            CompletionRingToTx {
                txs: tx_queues,
                comp,
                poller,
                assignment: assign::AssignGeneric {
                    frame: assign::AlignedFrame::new(frame_size),
                    index: assign::AlignedQueue::new(len),
                },
            }
            .await;
        }
        (len, true, false) => {
            trace!("using frame-aligned mode with {len} queues");
            CompletionRingToTx {
                txs: tx_queues,
                comp,
                poller,
                assignment: assign::AssignGeneric {
                    frame: assign::AlignedFrame::new(frame_size),
                    index: assign::UnalignedQueue::new(len),
                },
            }
            .await;
        }
        (len, false, true) => {
            trace!("using queue-aligned mode with {len} queues");
            CompletionRingToTx {
                txs: tx_queues,
                comp,
                poller,
                assignment: assign::AssignGeneric {
                    frame: assign::UnalignedFrame::new(frame_size),
                    index: assign::AlignedQueue::new(len),
                },
            }
            .await;
        }
        (len, false, false) => {
            trace!("using unaligned mode with {len} queues");
            CompletionRingToTx {
                txs: tx_queues,
                comp,
                poller,
                assignment: assign::AssignGeneric {
                    frame: assign::UnalignedFrame::new(frame_size),
                    index: assign::UnalignedQueue::new(len),
                },
            }
            .await;
        }
    }
}

/// Polls the completion queue for progress
pub trait Poller: Unpin {
    fn poll(&mut self, comp: &mut ring::Completion, cx: &mut Context) -> Poll<Option<u32>>;
    fn release(&mut self, comp: &mut ring::Completion, count: usize);
}

impl Poller for () {
    #[inline]
    fn poll(&mut self, comp: &mut ring::Completion, cx: &mut Context) -> Poll<Option<u32>> {
        // In this mode we are busy polling so wake ourselves up on every iteration
        cx.waker().wake_by_ref();

        // try to acquire entries from the completion queue
        let count = comp.acquire(1);

        trace!("acquired {count} items from the completion queue");

        if count > 0 {
            Poll::Ready(Some(count))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn release(&mut self, comp: &mut ring::Completion, count: usize) {
        trace!("releasing {count} items to the completion queue");
        if count > 0 {
            // release the number of consumed items to the completion queue
            comp.release(count as _);
        }
    }
}

impl Poller for worker::Receiver {
    #[inline]
    fn poll(&mut self, comp: &mut ring::Completion, cx: &mut Context) -> Poll<Option<u32>> {
        // try to acquire some work from the producers
        let credits = match self.poll_acquire(cx) {
            Poll::Ready(Some(count)) => count as u32,
            Poll::Ready(None) => {
                // there are no producers left so we're closing
                return Poll::Ready(None);
            }
            Poll::Pending => {
                // there's no work to be done so yield and wait for a producer to wake us up
                return Poll::Pending;
            }
        };

        trace!("acquired {credits} worker credits");

        // acquire entries from the completion queue
        let actual = comp.acquire(credits);
        trace!("acquired {actual} entries from the completion queue");

        // just in case there's a race between the work items count and the completion queue we'll
        // take the minimum here.
        let actual = actual.min(credits);

        // we need to make sure to wake back up so we can query to see if there's work to be done
        cx.waker().wake_by_ref();

        if actual > 0 {
            Poll::Ready(Some(actual))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn release(&mut self, comp: &mut ring::Completion, count: usize) {
        trace!("releasing {count} entries to the completion queue");

        if count > 0 {
            // release the number of consumed items to the completion queue
            comp.release(count as _);

            // mark `count` number of items as complete
            self.finish(count);
        }
    }
}

/// A group of TX queues that are responsible for filling packets
trait Txs: Unpin {
    /// Iterates over all of the queues in the group
    fn for_each<F: FnMut(&mut Sender)>(&mut self, f: F);
    /// Returns the number of queues
    fn len(&self) -> usize;
}

impl Txs for Vec<Sender> {
    #[inline]
    fn for_each<F: FnMut(&mut Sender)>(&mut self, mut f: F) {
        for s in self.iter_mut() {
            f(s);
        }
    }

    #[inline]
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl Txs for Sender {
    #[inline]
    fn for_each<F: FnMut(&mut Sender)>(&mut self, mut f: F) {
        f(self);
    }

    #[inline]
    fn len(&self) -> usize {
        1
    }
}

struct CompletionRingToTx<T: Txs, A: assign::Assign, P: Poller> {
    txs: T,
    comp: ring::Completion,
    poller: P,
    assignment: A,
}

impl<T: Txs, A: assign::Assign, P: Poller> Future for CompletionRingToTx<T, A, P> {
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let Self {
            txs,
            comp,
            poller,
            assignment,
        } = self.get_mut();

        trace!("polling completion ring to tx");

        // try to query if we have any ready items
        let count = match poller.poll(comp, cx) {
            Poll::Ready(Some(count)) => {
                // we're ready, keep going
                count
            }
            Poll::Ready(None) => {
                // shut down the task
                return Poll::Ready(());
            }
            Poll::Pending => {
                // nothing to do right now
                return Poll::Pending;
            }
        };

        let (head, tail) = comp.data();

        let mut sent = 0;
        let mut closed = 0;

        let mut idx = 0;
        txs.for_each(|tx| {
            match tx.try_slice() {
                Ok(Some(mut slice)) => {
                    /// copies the completion items into the worker's queue
                    macro_rules! extend {
                        ($name:ident) => {
                            if !$name.is_empty() {
                                let mut iter = $name
                                    .iter()
                                    .take(count as _)
                                    .copied()
                                    .filter(|desc| assignment.assign(*desc, idx))
                                    .map(|desc| {
                                        trace!("assigning address {} to queue {idx}", desc.address);

                                        sent += 1;
                                        desc
                                    })
                                    .peekable();

                                while iter.peek().is_some() {
                                    if slice.extend(&mut iter).is_err() {
                                        trace!("tx queue {idx} is closed");
                                        closed += 1;
                                        idx += 1;
                                        return;
                                    }
                                }
                            }
                        };
                    }

                    extend!(head);
                    extend!(tail);
                }
                Ok(None) => {
                    unreachable!("tx queue capacity should exceed that of the completion queue");
                }
                Err(_) => {
                    trace!("tx queue {idx} closed");
                    closed += 1;
                }
            }

            idx += 1;
        });

        // let the poller know how many items we consumed
        poller.release(comp, sent);

        // if all of the queues are closed then shut down the task
        if closed == txs.len() {
            trace!("all tx queues closed; shutting down");
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

    async fn execute_test(workers: usize, frame_size: u32) {
        let channel_size = QUEUE_SIZE;
        let worker_total = TEST_ITEMS / workers;
        let expected_total = worker_total * workers;

        let mut worker_channels = vec![];
        let mut worker_done = vec![];

        for idx in 0..workers {
            let (rx_send, mut rx_recv) = spsc::channel::<UmemDescriptor>(channel_size);
            let (done_send, done_recv) = oneshot::channel();

            worker_channels.push(rx_send);
            worker_done.push(done_recv);

            tokio::spawn(async move {
                let mut total = 0;
                let mut expected = (idx as u64..)
                    .step_by(workers)
                    .map(|v| v * frame_size as u64);

                while rx_recv.acquire().await.is_ok() {
                    let mut slice = rx_recv.slice();

                    while let Some(entry) = slice.pop() {
                        trace!("queue {idx} received address {}", entry.address);

                        assert_eq!(
                            entry.address,
                            expected.next().unwrap(),
                            "address does not match the expected value"
                        );
                        total += 1;
                    }
                }

                trace!("all queue items for {idx} received; shutting down");

                done_send.send(total).unwrap();
            });
        }

        let (ring_rx, mut ring_tx) = ring::testing::completion_fill(channel_size as u32);
        let (worker_send, worker_recv) = worker::channel();

        tokio::spawn(completion_to_tx(
            worker_recv,
            ring_rx,
            frame_size,
            worker_channels,
        ));

        tokio::spawn(async move {
            let mut addresses = (0..expected_total as u64)
                .map(|address| UmemDescriptor {
                    address: address * frame_size as u64,
                })
                .peekable();

            let mut total = 0;

            while addresses.peek().is_some() {
                let count = ring_tx.acquire(1);

                trace!("acquired {count} TX ring entries");

                if count == 0 {
                    random_delay().await;
                    continue;
                }

                let batch_size = thread_rng().gen_range(1..=count);
                trace!("TX batch size set to {batch_size}");

                let (head, tail) = ring_tx.data();

                let mut sent = 0;
                for (desc, dest) in (&mut addresses)
                    .take(batch_size as _)
                    .zip(head.iter_mut().chain(tail))
                {
                    trace!("sending address {}", desc.address);
                    *dest = desc;
                    sent += 1;
                }

                trace!("sent {sent} items");

                ring_tx.release(sent as _);
                worker_send.submit(sent);
                total += sent;

                random_delay().await;
            }

            trace!("all items sent; shutting down");

            assert_eq!(total, expected_total);
        });

        let mut actual_total = 0;

        for done_recv in worker_done {
            actual_total += done_recv.await.unwrap();
        }

        assert_eq!(expected_total as u64, actual_total);
    }

    #[tokio::test]
    async fn single_worker() {
        execute_test(1, 4096).await;
    }

    #[tokio::test]
    async fn multiple_worker_aligned() {
        execute_test(4, 16).await;
    }

    #[tokio::test]
    async fn multiple_worker_unaligned() {
        execute_test(4, 17).await;
    }
}
