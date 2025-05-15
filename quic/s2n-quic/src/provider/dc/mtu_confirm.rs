// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Connection;
use s2n_quic_core::{
    ensure,
    event::{
        api as events,
        api::{ConnectionInfo, ConnectionMeta, MtuUpdated, Subscriber},
    },
};
use std::io;
use tokio::sync::watch;

/// `event::Subscriber` used for ensuring an s2n-quic client or server negotiating dc
/// waits for post-handshake MTU probing to complete
pub struct MtuConfirmComplete;

impl MtuConfirmComplete {
    /// Blocks the task until the provided connection has either completed MTU probing or closed
    pub async fn wait_ready(conn: &mut Connection) -> io::Result<()> {
        let mut receiver = conn
            .query_event_context_mut(|context: &mut MtuConfirmContext| context.sender.subscribe())
            .map_err(io::Error::other)?;

        loop {
            match &*receiver.borrow_and_update() {
                // if we're ready then let the application know
                State::Ready => return Ok(()),
                State::Waiting => {}
            }

            if receiver.changed().await.is_err() {
                return Err(io::Error::other("never reached terminal state"));
            }
        }
    }
}

pub struct MtuConfirmContext {
    sender: watch::Sender<State>,
}

impl Default for MtuConfirmContext {
    fn default() -> Self {
        let (sender, _receiver) = watch::channel(State::default());
        Self { sender }
    }
}

impl MtuConfirmContext {
    /// Updates the state on the context
    fn update(&mut self, state: State) {
        self.sender.send_replace(state);
    }
}

impl Drop for MtuConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        self.sender.send_modify(|state| {
            if matches!(state, State::Waiting) {
                *state = State::Ready
            }
        });
    }
}

#[derive(Default)]
enum State {
    #[default]
    Waiting,
    Ready,
}

impl Subscriber for MtuConfirmComplete {
    type ConnectionContext = MtuConfirmContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        MtuConfirmContext::default()
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        _event: &events::ConnectionClosed,
    ) {
        ensure!(matches!(*context.sender.borrow(), State::Waiting));

        // The connection closed before MTU probing completed
        context.update(State::Ready);
    }

    #[inline]
    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &MtuUpdated,
    ) {
        ensure!(matches!(*context.sender.borrow(), State::Waiting));

        if event.search_complete {
            context.update(State::Ready)
        }
    }
}
