// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Driver;
use crate::{ring, socket, syscall};
use core::task::{Context, Poll};

type Fd = tokio::io::unix::AsyncFd<socket::Fd>;

/// Polls read readiness for a tokio socket
#[inline]
fn poll(fd: &Fd, rx: &mut ring::Rx, fill: &mut ring::Fill, cx: &mut Context) -> Option<u32> {
    // iterate twice to avoid race conditions on waker registration
    for _ in 0..2 {
        let mut count = rx.acquire(u32::MAX);
        count = fill.acquire(count).min(count);

        // if we have entries in the rings, then return
        if count > 0 {
            return Some(count);
        }

        match fd.poll_read_ready(cx) {
            Poll::Ready(Ok(mut guard)) => {
                // since we don't have any entries, clear the readiness and try again
                guard.clear_ready();
                continue;
            }
            Poll::Ready(Err(_)) => {
                // the fd is no longer registered so shut down the task
                return None;
            }
            Poll::Pending => {
                // put the task to sleep until tokio wakes it up with Rx progress
                return Some(0);
            }
        }
    }

    // If we got here tokio said the socket was ready to read, even though the ring is pending. In
    // this case, we'll manually call the socket's busy poll method and wake up again to try to
    // acquire more items.
    //
    // It's very unlikely this happens, but it's good to have just in case so we don't occupy all
    // of the worker's cycles.
    let _ = syscall::busy_poll(fd.get_ref());
    cx.waker().wake_by_ref();

    Some(0)
}

/// Polling implementation for an asynchronous socket
impl Driver for Fd {
    #[inline]
    fn poll(&mut self, rx: &mut ring::Rx, fill: &mut ring::Fill, cx: &mut Context) -> Option<u32> {
        poll(self, rx, fill, cx)
    }
}

/// Polling implementation for a shared asynchronous socket
impl Driver for std::sync::Arc<Fd> {
    #[inline]
    fn poll(&mut self, rx: &mut ring::Rx, fill: &mut ring::Fill, cx: &mut Context) -> Option<u32> {
        poll(self, rx, fill, cx)
    }
}
