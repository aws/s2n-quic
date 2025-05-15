// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Connection;
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
use tokio::sync::watch;

/// `event::Subscriber` used for ensuring an s2n-quic client or server negotiating dc
/// waits for the dc handshake to complete
pub struct ConfirmComplete;
impl ConfirmComplete {
    /// Blocks the task until the provided connection has either completed the dc handshake or closed
    /// with an error
    pub async fn wait_ready(conn: &mut Connection) -> io::Result<()> {
        let mut receiver = conn
            .query_event_context_mut(|context: &mut ConfirmContext| context.sender.subscribe())
            .map_err(io::Error::other)?;

        loop {
            match &*receiver.borrow_and_update() {
                // if we're ready or have errored then let the application know
                State::Ready => return Ok(()),
                State::Failed(error) => return Err((*error).into()),
                State::Waiting(_) => {}
            }

            if receiver.changed().await.is_err() {
                return Err(io::Error::other("never reached terminal state"));
            }
        }
    }
}

pub struct ConfirmContext {
    sender: watch::Sender<State>,
}

impl Default for ConfirmContext {
    fn default() -> Self {
        let (sender, _receiver) = watch::channel(State::default());
        Self { sender }
    }
}

impl ConfirmContext {
    /// Updates the state on the context
    fn update(&mut self, state: State) {
        self.sender.send_replace(state);
    }
}

impl Drop for ConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        self.sender.send_modify(|state| {
            if matches!(state, State::Waiting(_)) {
                *state = State::Failed(connection::Error::unspecified());
            }
        });
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
        ensure!(matches!(*context.sender.borrow(), State::Waiting(_)));
        let is_ready = matches!(
            *context.sender.borrow(),
            State::Waiting(Some(DcState::PathSecretsReady { .. }))
        );

        match (&meta.endpoint_type, event.error, is_ready) {
            (EndpointType::Server { .. }, Error::Closed { .. }, true) => {
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
        ensure!(matches!(*context.sender.borrow(), State::Waiting(_)));

        match event.state {
            DcState::NoVersionNegotiated { .. } => context.update(State::Failed(
                Error::invalid_configuration("peer does not support specified dc versions"),
            )),
            DcState::Complete { .. } => {
                // notify the application that the dc handshake has completed
                context.update(State::Ready);
            }
            _ => {
                context.update(State::Waiting(Some(event.state.clone())));
            }
        }
    }
}
