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

enum CloseState {
    /// Send a close request to the endpoint
    SendCloseAttempt {
        close_sender: CloseSender,
        is_open: Arc<AtomicBool>,
    },
    /// Wait for the endpoint to gracefully close and notify this future
    AwaitEndpointClose {
        is_open: Arc<AtomicBool>,
    },
    Unreachable,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Attempt {
    state: CloseState,
}

impl Attempt {
    /// Creates a Close attempt
    pub fn new(closer: Closer) -> Self {
        Self {
            state: CloseState::SendCloseAttempt {
                close_sender: closer.close_sender,
                is_open: closer.is_open,
            },
        }
    }
}

impl Future for Attempt {
    type Output = Result<(), connection::Error>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        loop {
            match core::mem::replace(&mut self.state, CloseState::Unreachable) {
                CloseState::SendCloseAttempt {
                    mut close_sender,
                    is_open,
                } => {
                    if is_open.load(Ordering::SeqCst) {
                        let waker = cx.waker().clone();
                        match close_sender.poll_ready(cx) {
                            Poll::Ready(Ok(())) => {
                                // send a waker to the endpoint, which is woken once the endpoint has closed
                                match close_sender.try_send(waker) {
                                    Ok(_) => {
                                        self.state = CloseState::AwaitEndpointClose { is_open };
                                    }
                                    Err(err) if err.is_full() => {
                                        // reset to the original state
                                        self.state = CloseState::SendCloseAttempt {
                                            close_sender,
                                            is_open,
                                        };

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
                                // reset to the original state
                                self.state = CloseState::SendCloseAttempt {
                                    close_sender,
                                    is_open,
                                };
                            }
                        }
                    } else {
                        // the endpoint is already closed so return
                        return Poll::Ready(Ok(()));
                    }

                    return Poll::Pending;
                }
                CloseState::AwaitEndpointClose { is_open } => {
                    if is_open.load(Ordering::SeqCst) {
                        self.state = CloseState::AwaitEndpointClose { is_open };
                        return Poll::Pending;
                    } else {
                        return Poll::Ready(Ok(()));
                    }
                }
                CloseState::Unreachable => {
                    unreachable!(
                        "Unreachable is an immediate state and should not exist across polls"
                    );
                }
            }
        }
    }
}
