// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

type Fd = tokio::io::unix::AsyncFd<socket::Fd>;

/// Notifies the socket that the TX queue has no capacity
#[inline]
fn notify_empty(fd: &Fd, tx: &mut ring::Tx, cx: &mut Context) -> Poll<()> {
    // only notify the socket if it's set the needs wakeup flag
    if !tx.needs_wakeup() {
        trace!("TX ring doesn't need wake, returning early");
        return Poll::Ready(());
    }

    // limit the number of loops to prevent endless spinning on registering wakers
    for iteration in 0..10 {
        trace!("iteration {}", iteration);

        // query socket readiness through tokio's polling facilities
        match fd.poll_write_ready(cx) {
            Poll::Ready(Ok(mut guard)) => {
                // try to acquire entries for the queue
                let count = tx.acquire(u32::MAX) as usize;

                trace!("acquired {count} items from TX ring");

                // if we didn't acquire all of the capacity, we need to clear readiness and try again
                if count != tx.capacity() {
                    guard.clear_ready();

                    // check to see if we need to wake up the socket again
                    if tx.needs_wakeup() {
                        let _ = syscall::wake_tx(guard.get_ref());
                    }

                    trace!("clearing socket readiness and trying again");
                    continue;
                }

                return Poll::Ready(());
            }
            Poll::Ready(Err(err)) => {
                trace!("socket returned an error while polling: {err:?}; closing poller");
                return Poll::Ready(());
            }
            Poll::Pending => {
                trace!("ring out of items; sleeping");
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

/// Polling implementation for an asynchronous socket
impl Notifier for Fd {
    #[inline]
    fn notify(&mut self, tx: &mut ring::Tx, cx: &mut Context, _count: u32) {
        // try making progress on the socket regardless of transmission count
        let _ = self.notify_empty(tx, cx);
    }

    #[inline]
    fn notify_empty(&mut self, tx: &mut ring::Tx, cx: &mut Context) -> Poll<()> {
        notify_empty(self, tx, cx)
    }
}

/// Polling implementation for a shared asynchronous socket
impl Notifier for std::sync::Arc<Fd> {
    #[inline]
    fn notify(&mut self, tx: &mut ring::Tx, cx: &mut Context, _count: u32) {
        // try making progress on the socket regardless of transmission count
        let _ = self.notify_empty(tx, cx);
    }

    #[inline]
    fn notify_empty(&mut self, tx: &mut ring::Tx, cx: &mut Context) -> Poll<()> {
        notify_empty(self, tx, cx)
    }
}
