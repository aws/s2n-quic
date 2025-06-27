// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    endpoint,
    sync::{flag, flag::Writer},
};
use s2n_quic_core::{
    ack,
    crypto::tls,
    dc,
    dc::{Endpoint, Path},
    ensure, event,
    event::builder::{DcState, DcStateChanged},
    frame::DcStatelessResetTokens,
    packet::number::PacketNumber,
    state::{event, is},
    stateless_reset, transmission,
    transmission::interest::Query,
    transport,
};

/// Manages transmission and receipt of `DC_STATELESS_RESET_TOKENS` and
/// notifications to the dc::Endpoint Path
pub struct Manager<Config: endpoint::Config> {
    path: Option<<<Config as endpoint::Config>::DcEndpoint as Endpoint>::Path>,
    version: Option<dc::Version>,
    state: State,
    stateless_reset_token_sync: Flag,
}

type Flag = flag::Flag<DcStatelessResetTokenWriter>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    InitServer,
    InitClient,
    ServerPathSecretsReady,
    ClientPathSecretsReady,
    ServerTokensSent,
    Complete,
}

impl State {
    is!(is_init, InitServer | InitClient);
    is!(
        is_path_secrets_ready,
        ServerPathSecretsReady | ClientPathSecretsReady
    );
    is!(is_server_tokens_sent, ServerTokensSent);
    is!(is_complete, Complete);

    event! {
        on_path_secrets_ready(
            InitServer => ServerPathSecretsReady,
            InitClient => ClientPathSecretsReady
        );
        on_peer_stateless_reset_tokens(
            ClientPathSecretsReady => Complete,
            ServerPathSecretsReady => ServerTokensSent
        );
        on_stateless_reset_tokens_acked(ServerTokensSent => Complete);
    }
}

impl<Config: endpoint::Config> Manager<Config> {
    /// Constructs a new `dc::Manager` with the optional given path
    ///
    /// If path is `None`, the `dc::Manager` will be disabled
    pub fn new<Pub: event::ConnectionPublisher>(
        path: Option<<<Config as endpoint::Config>::DcEndpoint as Endpoint>::Path>,
        version: dc::Version,
        publisher: &mut Pub,
    ) -> Self {
        ensure!(path.is_some(), Self::disabled());

        publisher.on_dc_state_changed(DcStateChanged {
            state: DcState::VersionNegotiated { version },
        });
        let state = if Config::ENDPOINT_TYPE.is_server() {
            State::InitServer
        } else {
            State::InitClient
        };

        Self {
            path,
            version: Some(version),
            state,
            stateless_reset_token_sync: Flag::default(),
        }
    }

    /// Returns a disabled `dc::Manager`
    pub fn disabled() -> Self {
        Self {
            path: None,
            version: None,
            state: State::Complete,
            stateless_reset_token_sync: Flag::default(),
        }
    }

    /// The dc version that was negotiated, if any
    ///
    /// Returns `None` if no version was negotiated or the `dc::Endpoint` did
    /// not initialize a path for the connection
    pub fn version(&self) -> Option<dc::Version> {
        self.version
    }

    /// Called when the TLS session has indicated path secrets are ready
    /// to be derived for the dc path
    ///
    /// Initiates sending of the `DC_STATELESS_RESET_TOKENS` frame on the client
    pub fn on_path_secrets_ready<Pub: event::ConnectionPublisher>(
        &mut self,
        session: &impl tls::TlsSession,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        ensure!(self.path.is_some(), Ok(()));
        ensure!(
            self.state.on_path_secrets_ready().is_ok(),
            Err(transport::Error::INTERNAL_ERROR)
        );

        let tokens = self.path.on_path_secrets_ready(session)?;
        let flag = Flag::new(DcStatelessResetTokenWriter::new(tokens));
        self.stateless_reset_token_sync = flag;

        if Config::ENDPOINT_TYPE.is_client() {
            // Start sending the `DC_STATELESS_RESET_TOKENS` frame on the client
            // The server will wait until it has received tokens from the client
            // before sending its own, though typically this will happen immediately
            // after path secrets are ready
            self.stateless_reset_token_sync.send();
        }

        publisher.on_dc_state_changed(DcStateChanged {
            state: DcState::PathSecretsReady,
        });

        Ok(())
    }

    /// Called when a `DC_STATELESS_RESET_TOKENS` frame is received from the peer
    ///
    /// On the client, this completes the dc path handshake
    /// On the server, this initiates sending the server's `DC_STATELESS_RESET_TOKENS` frame
    pub fn on_peer_dc_stateless_reset_tokens<'a, Pub: event::ConnectionPublisher>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
        publisher: &mut Pub,
    ) {
        ensure!(self.state.on_peer_stateless_reset_tokens().is_ok());

        self.path
            .on_peer_stateless_reset_tokens(stateless_reset_tokens);
        if Config::ENDPOINT_TYPE.is_server() {
            self.stateless_reset_token_sync.send();
        } else {
            self.path.on_dc_handshake_complete();
            publisher.on_dc_state_changed(DcStateChanged {
                state: DcState::Complete,
            });
        }
    }

    /// Called when a range of packets have been acknowledged
    pub fn on_packet_ack<A: ack::Set, Pub: event::ConnectionPublisher>(
        &mut self,
        ack_set: &A,
        publisher: &mut Pub,
    ) {
        ensure!(self.stateless_reset_token_sync.on_packet_ack(ack_set));
        ensure!(self.state.on_stateless_reset_tokens_acked().is_ok());

        debug_assert!(Config::ENDPOINT_TYPE.is_server());
        self.path.on_dc_handshake_complete();
        publisher.on_dc_state_changed(DcStateChanged {
            state: DcState::Complete,
        });
    }

    /// Called when a range of packets has been lost
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.stateless_reset_token_sync.on_packet_loss(ack_set);
    }

    /// Called when the MTU of the path has changed
    pub fn on_mtu_updated(&mut self, max_datagram_size: u16) {
        self.path.on_mtu_updated(max_datagram_size)
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn path(&self) -> &<<Config as endpoint::Config>::DcEndpoint as Endpoint>::Path {
        self.path.as_ref().expect("path should be specified")
    }
}

impl<Config: endpoint::Config> transmission::Provider for Manager<Config> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let _ = self.stateless_reset_token_sync.on_transmit(context);
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for Manager<Config> {
    fn transmission_interest<Q: Query>(&self, query: &mut Q) -> transmission::interest::Result {
        let result = self.stateless_reset_token_sync.transmission_interest(query);
        #[cfg(debug_assertions)]
        if result.is_err() {
            if Config::ENDPOINT_TYPE.is_server() {
                // The server should only have transmission interest in the server tokens sent state
                assert!(self.state.is_server_tokens_sent());
            } else {
                // The client should only have transmission interest in the path secrets are ready state
                assert!(self.state.is_path_secrets_ready());
            }
        }
        result
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DcStatelessResetTokenWriter {
    tokens: Vec<stateless_reset::Token>,
}

impl DcStatelessResetTokenWriter {
    fn new(tokens: Vec<stateless_reset::Token>) -> Self {
        Self { tokens }
    }
}

impl Writer for DcStatelessResetTokenWriter {
    fn write_frame<W: WriteContext>(&mut self, context: &mut W) -> Option<PacketNumber> {
        match DcStatelessResetTokens::new(self.tokens.as_slice()) {
            Ok(frame) => context.write_frame(&frame),
            Err(error) => {
                debug_assert!(
                    false,
                    "The dc provider produced invalid stateless reset tokens: {error}"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;
