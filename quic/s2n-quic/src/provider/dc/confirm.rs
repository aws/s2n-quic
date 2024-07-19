// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Connection;
use core::task::{Context, Poll, Waker};
use s2n_quic_core::{
    connection,
    connection::Error,
    ensure,
    event::{
        api as events,
        api::{ConnectionInfo, ConnectionMeta, DcState, EndpointType, Subscriber},
    },
};
use std::io;

/// `event::Subscriber` used for ensuring an s2n-quic client or server negotiating dc
/// waits for the dc handshake to complete
pub struct ConfirmComplete;
impl ConfirmComplete {
    /// Blocks the task until the provided connection has either completed the dc handshake or closed
    /// with an error
    pub async fn wait_ready(conn: &mut Connection) -> io::Result<()> {
        core::future::poll_fn(|cx| {
            conn.query_event_context_mut(|context: &mut ConfirmContext| context.poll_ready(cx))
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?
        })
        .await
    }
}

#[derive(Default)]
pub struct ConfirmContext {
    waker: Option<Waker>,
    state: State,
}

impl ConfirmContext {
    /// Updates the state on the context
    fn update(&mut self, state: State) {
        self.state = state;

        // notify the application that the state was updated
        self.wake();
    }

    /// Polls the context for handshake completion
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.state {
            // if we're ready or have errored then let the application know
            State::Ready => Poll::Ready(Ok(())),
            State::Failed(error) => Poll::Ready(Err(error.into())),
            State::Waiting(_) => {
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

impl Drop for ConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        if matches!(self.state, State::Waiting(_)) {
            self.state = State::Failed(connection::Error::unspecified());
        }
        self.wake();
    }
}

enum State {
    Waiting(Option<DcState>),
    Ready,
    Failed(connection::Error),
}

impl Default for State {
    fn default() -> Self {
        State::Waiting(None)
    }
}

impl Subscriber for ConfirmComplete {
    type ConnectionContext = ConfirmContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        ConfirmContext::default()
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &ConnectionMeta,
        event: &events::ConnectionClosed,
    ) {
        ensure!(matches!(context.state, State::Waiting(_)));

        match (&meta.endpoint_type, event.error, &context.state) {
            (
                EndpointType::Server { .. },
                Error::Closed { .. },
                State::Waiting(Some(DcState::PathSecretsReady { .. })),
            ) => {
                // The client may close the connection immediately after the dc handshake completes,
                // before it sends acknowledgement of the server's DC_STATELESS_RESET_TOKENS.
                // Since the server has already moved into the PathSecretsReady state, this can be considered
                // as a successful completion of the dc handshake.
                context.update(State::Ready)
            }
            _ => context.update(State::Failed(event.error)),
        }
    }

    #[inline]
    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::DcStateChanged,
    ) {
        ensure!(matches!(context.state, State::Waiting(_)));

        if let DcState::Complete { .. } = event.state {
            // notify the application that the dc handshake has completed
            context.update(State::Ready);
        } else {
            context.update(State::Waiting(Some(event.state.clone())));
        }
    }
}
