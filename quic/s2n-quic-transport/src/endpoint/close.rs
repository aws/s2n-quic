// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::handle::CloseReceiver;
use crate::{connection, endpoint::handle::CloseSender};
use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

/// Held by library. Used to receive close attempts and track close state.
#[derive(Debug)]
pub(crate) struct CloseHandle {
    /// Used to determine if the application has interest in closing the endpoint
    first_waker: Option<Waker>,
    /// A channel which is used to receive connection close attempts
    close_receiver: CloseReceiver,
    /// Track the endpoint open state
    endpoint_state: EndpointState,
}

impl CloseHandle {
    pub fn new(close_receiver: CloseReceiver, endpoint_state: EndpointState) -> Self {
        Self {
            first_waker: None,
            close_receiver,
            endpoint_state,
        }
    }

    /// Returns `Poll::Ready` if there is interest in closing the endpoint.
    pub fn poll_interest(&mut self) -> Poll<()> {
        if self.first_waker.is_some() {
            Poll::Ready(())
        } else {
            match self.close_receiver.try_recv() {
                Ok(waker) => {
                    self.first_waker = Some(waker);
                    Poll::Ready(())
                }
                _ => Poll::Pending,
            }
        }
    }

    /// Marks that the endpoint has finished processing and accepting connections and is
    /// ready to be closed.
    pub fn close(&mut self) {
        self.endpoint_state.close();

        if let Some(waker) = self.first_waker.take() {
            waker.wake();
        }
        while let Ok(waker) = self.close_receiver.try_recv() {
            waker.wake_by_ref();
        }
    }
}

/// Track if the endpoint is still open and handling connections
#[derive(Clone, Debug)]
pub(crate) struct EndpointState(Arc<AtomicBool>);

impl Default for EndpointState {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl EndpointState {
    /// Return `true` if the endpoint is still handling or accepting new connections
    fn is_open(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Marks that the endpoint has finished processing and accepting connections and is
    /// ready to be closed.
    fn close(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Closer {
    request_sent: bool,
    close_sender: CloseSender,
    endpoint_state: EndpointState,
}

impl Closer {
    /// Creates a Close attempt
    pub fn new(close_sender: CloseSender, endpoint_state: EndpointState) -> Self {
        Self {
            request_sent: false,
            close_sender,
            endpoint_state,
        }
    }

    pub(crate) fn poll_close(
        &mut self,
        context: &mut Context,
    ) -> Poll<Result<(), connection::Error>> {
        if !self.endpoint_state.is_open() {
            return Poll::Ready(Ok(()));
        }

        if !self.request_sent {
            match self.close_sender.poll_ready(context) {
                Poll::Ready(Ok(())) => {
                    // send a waker to the endpoint, which is woken once the endpoint has closed
                    match self.close_sender.try_send(context.waker().clone()) {
                        Ok(_) => {
                            self.request_sent = true;
                        }
                        Err(err) if err.is_full() => {
                            // yield and wake up the task since the opener misreported its ready state
                            context.waker().wake_by_ref();
                        }
                        Err(_) => {
                            // the endpoint is closed so return
                            return Poll::Ready(Ok(()));
                        }
                    }

                    return Poll::Pending;
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

        Poll::Pending
    }
}
