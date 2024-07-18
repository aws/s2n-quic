// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Connection;
use core::task::{Context, Poll, Waker};
use s2n_quic_core::{
    connection,
    event::{
        api as events,
        api::{ConnectionInfo, ConnectionMeta, Subscriber},
    },
};
use std::io;

/// `event::Subscriber` used for ensuring an s2n-quic client negotiating dc
/// waits for the dc handshake to complete
pub struct ClientConfirm;

impl ClientConfirm {
    /// Blocks the task until the provided connection has either completed the dc handshake or closed
    /// with an error
    pub async fn wait_ready(conn: &mut Connection) -> io::Result<()> {
        core::future::poll_fn(|cx| {
            conn.query_event_context_mut(|context: &mut ClientConfirmContext| {
                context.poll_ready(cx)
            })
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
        })
        .await
    }
}

#[derive(Default)]
pub struct ClientConfirmContext {
    waker: Option<Waker>,
    state: State,
}

impl ClientConfirmContext {
    /// Updates the state on the context
    fn update(&mut self, state: State) {
        self.state = state;

        // notify the application that the state was updated
        self.wake();
    }

    /// Polls the context for handshake confirmation
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.state {
            // if we're ready or have errored then let the application know
            State::Ready => Poll::Ready(Ok(())),
            State::Failed(error) => Poll::Ready(Err(error.into())),
            State::Waiting => {
                // store the waker so we can notify the application of state updates
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    /// notify the application of a state update
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Drop for ClientConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        if matches!(self.state, State::Waiting) {
            self.state = State::Failed(connection::Error::unspecified());
        }
        self.wake();
    }
}

#[derive(Default)]
enum State {
    #[default]
    Waiting,
    Ready,
    Failed(connection::Error),
}

impl Subscriber for ClientConfirm {
    type ConnectionContext = ClientConfirmContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        ClientConfirmContext::default()
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _: &ConnectionMeta,
        event: &events::ConnectionClosed,
    ) {
        // notify the application if we close the connection
        context.update(State::Failed(event.error));
    }

    #[inline]
    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::DcStateChanged,
    ) {
        if let events::DcState::Complete { .. } = event.state {
            // notify the application that the dc handshake has completed
            context.update(State::Ready);
        }
    }
}
