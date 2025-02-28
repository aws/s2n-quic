// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::ring_deque::{self, RingDeque};
use core::{fmt, task::Poll};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Waker,
};

pub use ring_deque::{Closed, Priority};

pub fn new<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    assert!(cap >= 1, "capacity must be at least 2");

    let channel = Arc::new(Channel {
        queue: RingDeque::new(cap),
        sender_count: AtomicUsize::new(1),
    });

    let s = Sender {
        channel: channel.clone(),
    };
    let r = Receiver { channel };
    (s, r)
}

struct Channel<T> {
    queue: RingDeque<T, Option<Waker>>,
    sender_count: AtomicUsize,
}

impl<T> Channel<T> {
    /// Closes the channel and notifies all blocked operations.
    ///
    /// Returns `Err` if this call has closed the channel and it was not closed already.
    fn close(&self) -> Result<(), Closed> {
        self.queue.close()?;

        Ok(())
    }
}

/// A message sender
///
/// Note that this channel implementation does not allow for backpressure on the
/// sending rate. Instead, the queue is rotated to make room for new items and
/// returned to the sender.
pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    #[inline]
    pub fn send_back(&self, msg: T) -> Result<Option<T>, Closed> {
        let res = self.channel.queue.push_back(msg)?;

        Ok(res)
    }

    #[inline]
    pub fn send_front(&self, msg: T) -> Result<Option<T>, Closed> {
        let res = self.channel.queue.push_front(msg)?;

        Ok(res)
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Decrement the sender count and close the channel if it drops down to zero.
        if self.channel.sender_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = self.channel.close();
        }
    }
}

impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sender {{ .. }}")
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        let count = self.channel.sender_count.fetch_add(1, Ordering::Relaxed);

        // Make sure the count never overflows, even if lots of sender clones are leaked.
        assert!(count < usize::MAX / 2, "too many senders");

        Sender {
            channel: self.channel.clone(),
        }
    }
}

/// The receiving side of a channel.
///
/// When the receiver is dropped, the channel will be closed.
///
/// The channel can also be closed manually by calling [`Receiver::close()`].
pub struct Receiver<T> {
    // Inner channel state.
    channel: Arc<Channel<T>>,
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let _ = self.channel.close();
    }
}

impl<T> Receiver<T> {
    /// Attempts to receive a message from the front of the channel.
    ///
    /// If the channel is empty, or empty and closed, this method returns an error.
    #[inline]
    pub fn try_recv_front(&self) -> Result<Option<T>, Closed> {
        self.channel.queue.pop_front()
    }

    /// Attempts to receive a message from the back of the channel.
    ///
    /// If the channel is empty, or empty and closed, this method returns an error.
    #[inline]
    pub fn try_recv_back(&self) -> Result<Option<T>, Closed> {
        self.channel.queue.pop_back()
    }

    /// Receives a message from the front of the channel.
    ///
    /// If the channel is empty, this method waits until there is a message.
    ///
    /// If the channel is closed, this method receives a message or returns an error if there are
    /// no more messages.
    #[inline]
    pub async fn recv_front(&self) -> Result<T, Closed> {
        core::future::poll_fn(|cx| self.poll_recv_front(cx)).await
    }

    /// Receives a message from the front of the channel
    #[inline]
    pub fn poll_recv_front(&self, cx: &mut core::task::Context<'_>) -> Poll<Result<T, Closed>> {
        self.channel.queue.poll_pop_front(cx)
    }

    /// Receives a message from the back of the channel.
    ///
    /// If the channel is empty, this method waits until there is a message.
    ///
    /// If the channel is closed, this method receives a message or returns an error if there are
    /// no more messages.
    #[inline]
    pub async fn recv_back(&self) -> Result<T, Closed> {
        core::future::poll_fn(|cx| self.poll_recv_back(cx)).await
    }

    /// Receives a message from the back of the channel.
    #[inline]
    pub fn poll_recv_back(&self, cx: &mut core::task::Context<'_>) -> Poll<Result<T, Closed>> {
        self.channel.queue.poll_pop_back(cx)
    }

    /// Swaps the contents of the channel with the given deque.
    ///
    /// If the channel is closed, this method returns an error.
    #[inline]
    pub async fn swap(&self, out: &mut std::collections::VecDeque<T>) -> Result<(), Closed> {
        core::future::poll_fn(|cx| self.poll_swap(cx, out)).await
    }

    /// Swaps the contents of the channel with the given deque.
    ///
    /// If the channel is closed, this method returns an error. If the channel is currently
    /// empty, `Pending` will be returned.
    #[inline]
    pub fn poll_swap(
        &self,
        cx: &mut core::task::Context<'_>,
        out: &mut std::collections::VecDeque<T>,
    ) -> Poll<Result<(), Closed>> {
        self.channel.queue.poll_swap(cx, out)
    }

    /// Closes the channel for receiving
    #[inline]
    pub fn close(&self) -> Result<(), Closed> {
        self.channel.close()
    }
}

impl<T> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Receiver {{ .. }}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ext::*, sim, task};
    use std::time::Duration;

    #[test]
    fn test_unlimited() {
        sim(|| {
            let (tx, rx) = new(2);

            async move {
                for v in 0u64.. {
                    if tx.send_back(v).is_err() {
                        return;
                    };
                    // let the receiver read from the task
                    task::yield_now().await;
                }
            }
            .primary()
            .spawn();

            async move {
                for expected in 0u64..10 {
                    let actual = rx.recv_front().await.unwrap();
                    assert_eq!(actual, expected);
                }
            }
            .primary()
            .spawn();
        });
    }

    #[test]
    fn test_send_limited() {
        sim(|| {
            let (tx, rx) = new(2);

            async move {
                for v in 0u64.. {
                    if tx.send_back(v).is_err() {
                        return;
                    };
                    Duration::from_millis(1).sleep().await;
                }
            }
            .primary()
            .spawn();

            async move {
                for expected in 0u64..10 {
                    let actual = rx.recv_front().await.unwrap();
                    assert_eq!(actual, expected);
                }
            }
            .primary()
            .spawn();
        });
    }

    #[test]
    fn test_recv_limited() {
        sim(|| {
            let (tx, rx) = new(2);

            async move {
                for v in 0u64.. {
                    match tx.send_back(v) {
                        Ok(Some(_old)) => {
                            // the channel doesn't provide backpressure so we'll need to sleep
                            Duration::from_millis(1).sleep().await;
                        }
                        Ok(None) => {
                            continue;
                        }
                        Err(_) => {
                            // the receiver is done
                            return;
                        }
                    }
                }
            }
            .primary()
            .spawn();

            async move {
                let mut min = 0;
                for _ in 0u64..10 {
                    let actual = rx.recv_front().await.unwrap();
                    assert!(actual > min);
                    min = actual;
                    Duration::from_millis(1).sleep().await;
                }
            }
            .primary()
            .spawn();
        });
    }
}
