// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::primitive::{Arc, AtomicUsize, AtomicWaker, Ordering};
use cache_padded::CachePadded;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Creates a worker channel with a Sender and Receiver
pub fn channel() -> (Sender, Receiver) {
    let state = Arc::new(State::default());
    let sender = Sender(state.clone());
    let receiver = Receiver { state, credits: 0 };
    (sender, receiver)
}

/// A handle to the receiver side of the worker channel
///
/// This handle is used by the worker to wake up when there is work to do.
pub struct Receiver {
    state: Arc<State>,
    credits: usize,
}

impl Receiver {
    /// Acquires work to be processed for the Receiver
    ///
    /// `None` is returned when there are no more active Senders.
    #[inline]
    pub async fn acquire(&mut self) -> Option<usize> {
        Acquire(self).await
    }

    /// Polls work to be processed for the receiver
    ///
    /// `None` is returned when there are no more active Senders.
    #[inline]
    pub fn poll_acquire(&mut self, cx: &mut Context) -> Poll<Option<usize>> {
        let state = &*self.state;

        macro_rules! acquire {
            () => {{
                // take the credits that we've been given by the senders
                self.credits += state.remaining.swap(0, Ordering::Acquire);

                // if we have any credits then return
                if self.credits > 0 {
                    return Poll::Ready(Some(self.credits));
                }
            }};
        }

        // first try to acquire credits
        acquire!();

        // if we didn't get any credits then register the waker
        state.receiver.register(cx.waker());

        // make one last effort to acquire credits in case a sender submitted some while we were
        // registering the waker
        acquire!();

        // If we're the only ones with a handle to the state then we're done
        if state.senders.load(Ordering::Acquire) == 0 {
            return Poll::Ready(None);
        }

        Poll::Pending
    }

    /// Marks `count` jobs as finished
    #[inline]
    pub fn finish(&mut self, count: usize) {
        debug_assert!(self.credits >= count);
        // decrement the number of credits we have
        self.credits -= count;
    }
}

/// A handle to submit work to be done to a worker receiver
///
/// Multiple Sender handles can be created with `.clone()`.
#[derive(Clone)]
pub struct Sender(Arc<State>);

impl Sender {
    /// Submits `count` jobs to be executed by the worker receiver
    #[inline]
    pub fn submit(&self, count: usize) {
        let state = &*self.0;

        // increment the work counter
        state.remaining.fetch_add(count, Ordering::Release);

        // wake up the receiver if possible
        state.receiver.wake();
    }
}

impl Drop for Sender {
    #[inline]
    fn drop(&mut self) {
        let state = &*self.0;

        state.senders.fetch_sub(1, Ordering::Release);

        // wake up the receiver to notify that one of the senders has dropped
        state.receiver.wake();
    }
}

struct State {
    remaining: CachePadded<AtomicUsize>,
    receiver: AtomicWaker,
    senders: CachePadded<AtomicUsize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            remaining: Default::default(),
            receiver: Default::default(),
            senders: AtomicUsize::new(1).into(),
        }
    }
}

struct Acquire<'a>(&'a mut Receiver);

impl<'a> Future for Acquire<'a> {
    type Output = Option<usize>;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.0.poll_acquire(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::loom;

    fn loom_scenario(iterations: usize, send_batch_size: usize, recv_batch_size: usize) {
        assert_ne!(send_batch_size, 0);
        assert_ne!(recv_batch_size, 0);

        loom::model(move || {
            let (send, mut recv) = channel();

            let sender = loom::thread::spawn(move || {
                for _ in 0..iterations {
                    send.submit(send_batch_size);
                    loom::hint::spin_loop();
                }
            });

            let receiver = loom::thread::spawn(move || {
                loom::future::block_on(async move {
                    let mut total = 0;
                    while let Some(mut count) = recv.acquire().await {
                        assert_ne!(count, 0);

                        while count > 0 {
                            let to_finish = count.min(recv_batch_size);
                            recv.finish(to_finish);
                            total += to_finish;
                            count -= to_finish;
                        }
                    }

                    assert_eq!(total, iterations * send_batch_size);
                })
            });

            // loom tests will still run after returning so we don't need to join
            if cfg!(not(loom)) {
                sender.join().unwrap();
                receiver.join().unwrap();
            }
        });
    }

    /// Async loom tests seem to spin forever if the number of iterations is higher than 1.
    /// Ideally, this value would be a bit bigger to test more permutations of orderings.
    const ITERATIONS: usize = if cfg!(loom) { 1 } else { 100 };
    const SEND_BATCH_SIZE: usize = if cfg!(loom) { 2 } else { 8 };
    const RECV_BATCH_SIZE: usize = if cfg!(loom) { 2 } else { 8 };

    #[test]
    fn loom_no_items() {
        loom_scenario(0, 1, 1);
    }

    #[test]
    fn loom_single_item() {
        loom_scenario(ITERATIONS, 1, 1);
    }

    #[test]
    fn loom_send_batch() {
        loom_scenario(ITERATIONS, SEND_BATCH_SIZE, 1);
    }

    #[test]
    fn loom_recv_batch() {
        loom_scenario(ITERATIONS, 1, RECV_BATCH_SIZE);
    }

    #[test]
    fn loom_both_batch() {
        loom_scenario(ITERATIONS, SEND_BATCH_SIZE, RECV_BATCH_SIZE);
    }
}
