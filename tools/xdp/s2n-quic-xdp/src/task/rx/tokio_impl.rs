// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

type Fd = tokio::io::unix::AsyncFd<socket::Fd>;

/// Polls read readiness for a tokio socket
#[inline]
fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
    fd: &Fd,
    rx: &mut ring::Rx,
    cx: &mut Context,
    mut on_ready: F,
) -> Poll<Result<(), ()>> {
    // limit the number of loops to prevent endless spinning on registering wakers
    for iteration in 0..10 {
        trace!("iteration {}", iteration);

        // query socket readiness through tokio's polling facilities
        match fd.poll_read_ready(cx) {
            Poll::Ready(Ok(mut guard)) => {
                // try to acquire entries for the queue
                let count = rx.acquire(1) as usize;

                trace!("acquired {count} items from RX ring");

                // if we didn't get anything, we need to clear readiness and try again
                if count == 0 {
                    guard.clear_ready();
                    trace!("clearing socket readiness and trying again");
                    continue;
                }

                // we have at least one entry so notify the callback
                match on_ready(rx, cx) {
                    Some(actual) => {
                        trace!("consumed {actual} items");

                        // if we consumed all of the acquired items we'll need to poll the
                        // queue again for readiness so we can register a waker.
                        if actual >= count {
                            trace!("clearing socket readiness and trying again");
                            guard.clear_ready();
                        }

                        continue;
                    }
                    None => {
                        trace!("on_ready closed; closing receiver");

                        return Poll::Ready(Err(()));
                    }
                }
            }
            Poll::Ready(Err(err)) => {
                trace!("socket returned an error while polling: {err:?}; closing poller");
                return Poll::Ready(Err(()));
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
impl Poller for Fd {
    #[inline]
    fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
        &mut self,
        rx: &mut ring::Rx,
        cx: &mut Context,
        on_ready: F,
    ) -> Poll<Result<(), ()>> {
        poll(self, rx, cx, on_ready)
    }
}

/// Polling implementation for a shared asynchronous socket
impl Poller for std::sync::Arc<Fd> {
    #[inline]
    fn poll<F: FnMut(&mut ring::Rx, &mut Context) -> Option<usize>>(
        &mut self,
        rx: &mut ring::Rx,
        cx: &mut Context,
        on_ready: F,
    ) -> Poll<Result<(), ()>> {
        poll(self, rx, cx, on_ready)
    }
}
