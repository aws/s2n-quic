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
use std::time::Duration;
use tokio::sync::watch;

/// `event::Subscriber` used for ensuring an s2n-quic client or server negotiating dc
/// waits for post-handshake MTU probing to complete
pub struct MtuConfirmComplete;

impl MtuConfirmComplete {
    /// Blocks the task until the provided connection has either completed MTU probing or closed.
    ///
    /// If the peer doesn't support MtuProbingComplete, waits 1 second after local MTU probing
    /// completes to allow the peer to finish their probing.
    ///
    /// Returns whether the peer supports sending MtuProbingComplete frames.
    pub async fn wait_ready(conn: &mut Connection) {
        let (mut receiver, peer_will_send) = conn
            .query_event_context_mut(|context: &mut MtuConfirmContext| {
                (
                    context.sender.subscribe(),
                    context.peer_will_send_completion,
                )
            })
            .expect("connection context isn't properly set");

        loop {
            let ready = {
                let state = receiver.borrow_and_update();

                if peer_will_send {
                    // Wait for both local and remote completion
                    state.is_ready()
                } else {
                    // Only wait for local completion since peer won't send
                    state.local_ready
                }
            };

            if ready {
                // Peer didn't indicate they would send MtuProbingComplete.
                // Wait 1 second to allow the peer to finish their MTU probing.
                if !peer_will_send {
                    // s2n-quic testing module is using bach runtime, while it is using tokio runtime in production
                    #[cfg(any(test, feature = "unstable-provider-io-testing"))]
                    {
                        crate::provider::io::testing::time::delay(Duration::from_secs(1)).await;
                    }
                    #[cfg(not(any(test, feature = "unstable-provider-io-testing")))]
                    {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                return;
            }

            if receiver.changed().await.is_err() {
                // If the peer closes the connection, we return immediately since there's no point waiting.
                return;
            }
        }
    }
}

pub struct MtuConfirmContext {
    sender: watch::Sender<MtuProbingState>,
    peer_will_send_completion: bool,
}

impl Default for MtuConfirmContext {
    fn default() -> Self {
        let (sender, _receiver) = watch::channel(MtuProbingState::default());
        Self {
            sender,
            // Default to false in case that some users didn't deploy MtuProbingComplete frame feature.
            // If the feature is enabled, then this will always be overridden by mtu_probing_complete_support
            // transport parameter before it is used.
            peer_will_send_completion: false,
        }
    }
}

impl MtuConfirmContext {
    /// Updates the state and checks if both local and remote are complete
    fn update_and_check(&mut self, updater: impl FnOnce(&mut MtuProbingState)) {
        self.sender.send_modify(|state| {
            updater(state);
        });
    }
}

impl Drop for MtuConfirmContext {
    // make sure the application is notified that we're closing the connection
    fn drop(&mut self) {
        self.sender.send_modify(|state| {
            // Force ready state on connection close
            state.local_ready = true;
            state.remote_ready = true;
        });
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MtuProbingState {
    local_ready: bool,
    remote_ready: bool,
}

impl MtuProbingState {
    fn is_ready(&self) -> bool {
        self.local_ready && self.remote_ready
    }
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
    fn on_transport_parameters_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::TransportParametersReceived,
    ) {
        context.peer_will_send_completion = event.transport_parameters.mtu_probing_complete_support;
    }

    #[inline]
    fn on_connection_closed(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        _event: &events::ConnectionClosed,
    ) {
        let state = *context.sender.borrow();
        ensure!(!state.is_ready());

        // Log if peer indicated they would send MtuProbingComplete but never did
        if context.peer_will_send_completion && !state.remote_ready {
            tracing::warn!(
                local_ready = state.local_ready,
                "peer indicated MtuProbingComplete support but closed connection before sending it"
            );
        }

        // The connection closed before MTU probing completed
        // Force both to complete to unblock any waiting tasks
        context.update_and_check(|state| {
            state.local_ready = true;
            state.remote_ready = true;
        });
    }

    #[inline]
    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &MtuUpdated,
    ) {
        if event.search_complete {
            context.update_and_check(|state| {
                state.local_ready = true;
            });
        }
    }

    #[inline]
    fn on_mtu_probing_complete_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        _event: &events::MtuProbingCompleteReceived,
    ) {
        context.update_and_check(|state| {
            state.remote_ready = true;
        });
    }
}
