// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::handle::Closer;
use crate::{connection, endpoint::handle::CloseSender};
use alloc::sync::Arc;
use core::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
};

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Attempt {
    request_sent: bool,
    close_sender: CloseSender,
    is_open: Arc<AtomicBool>,
}

impl Attempt {
    /// Creates a Close attempt
    pub fn new(closer: Closer) -> Self {
        Self {
            request_sent: false,
            close_sender: closer.close_sender,
            is_open: closer.is_open,
        }
    }
}

impl Future for Attempt {
    type Output = Result<(), connection::Error>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        if !self.is_open.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }

        if !self.request_sent {
            loop {
                match self.close_sender.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        // send a waker to the endpoint, which is woken once the endpoint has closed
                        match self.close_sender.try_send(cx.waker().clone()) {
                            Ok(_) => {
                                self.request_sent = true;
                                return Poll::Pending;
                            }
                            Err(err) if err.is_full() => {
                                // yield and wake up the task since the opener mis-reported its ready state
                                cx.waker().wake_by_ref();
                            }
                            Err(_) => {
                                // the endpoint is closed so return
                                return Poll::Ready(Ok(()));
                            }
                        }
                    }
                    Poll::Ready(Err(_)) => {
                        // the endpoint is closed so return
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Pending => {
                        // pending capacity so try again
                    }
                }
            }
        }

        Poll::Pending
    }
}
