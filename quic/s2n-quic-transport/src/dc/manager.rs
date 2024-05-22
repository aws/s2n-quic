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
    state::is,
    stateless_reset, transmission,
    transmission::interest::Query,
};

/// Manages transmission and receipt of `DC_STATELESS_RESET_TOKENS` and
/// notifications to the dc::Endpoint Path
pub struct Manager<Config: endpoint::Config> {
    path: Option<<<Config as endpoint::Config>::DcEndpoint as Endpoint>::Path>,
    version: Option<dc::Version>,
    state: State,
}

type Flag = flag::Flag<DcStatelessResetTokenWriter>;

#[derive(Debug, Default)]
enum State {
    #[default]
    Init,
    PathSecretsReady(Flag),
    Complete,
}

impl State {
    is!(is_init, Init);
    is!(is_complete, Complete);
    fn is_path_secrets_ready(&self) -> bool {
        matches!(self, Self::PathSecretsReady(_))
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
        if path.is_some() {
            publisher.on_dc_state_changed(DcStateChanged {
                state: DcState::VersionNegotiated { version },
            });
            Self {
                path,
                version: Some(version),
                state: State::Init,
            }
        } else {
            Self::disabled()
        }
    }

    /// Returns a disabled `dc::Manager`
    pub fn disabled() -> Self {
        Self {
            path: None,
            version: None,
            state: State::Complete,
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
    /// Initiates sending of the `DC_STATELESS_RESET_TOKENS` frame
    pub fn on_path_secrets_ready<Pub: event::ConnectionPublisher>(
        &mut self,
        session: &impl tls::TlsSession,
        publisher: &mut Pub,
    ) {
        ensure!(!self.state.is_complete());

        self.path.on_path_secrets_ready(session);
        let mut flag = Flag::new(DcStatelessResetTokenWriter::new(
            self.path.stateless_reset_tokens(),
        ));
        flag.send();
        self.state = State::PathSecretsReady(flag);

        publisher.on_dc_state_changed(DcStateChanged {
            state: DcState::PathSecretsReady,
        })
    }

    /// Called when a `DC_STATELESS_RESET_TOKENS` frame is received from the peer
    ///
    /// For a client endpoint, this completes the dc path handshake
    pub fn on_peer_dc_stateless_reset_tokens<'a, Pub: event::ConnectionPublisher>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
        publisher: &mut Pub,
    ) {
        ensure!(self.state.is_path_secrets_ready());

        self.path
            .on_peer_stateless_reset_tokens(stateless_reset_tokens);

        if Config::ENDPOINT_TYPE.is_client() {
            self.state = State::Complete;
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
        if let State::PathSecretsReady(ref mut flag) = &mut self.state {
            if flag.on_packet_ack(ack_set) {
                self.state = State::Complete;
                publisher.on_dc_state_changed(DcStateChanged {
                    state: DcState::Complete,
                });
            }
        }
    }

    /// Called when a range of packets has been lost
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if let State::PathSecretsReady(ref mut flag) = &mut self.state {
            flag.on_packet_loss(ack_set);
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn path(&self) -> &<<Config as endpoint::Config>::DcEndpoint as Endpoint>::Path {
        self.path.as_ref().expect("path should be specified")
    }
}

impl<Config: endpoint::Config> transmission::Provider for Manager<Config> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if let State::PathSecretsReady(ref mut flag) = &mut self.state {
            let _ = flag.on_transmit(context);
        }
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for Manager<Config> {
    fn transmission_interest<Q: Query>(&self, query: &mut Q) -> transmission::interest::Result {
        match &self.state {
            State::PathSecretsReady(flag) => flag.transmission_interest(query),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DcStatelessResetTokenWriter {
    tokens: Vec<stateless_reset::Token>,
}

impl DcStatelessResetTokenWriter {
    fn new(tokens: &[stateless_reset::Token]) -> Self {
        Self {
            tokens: tokens.to_vec(),
        }
    }
}

impl Writer for DcStatelessResetTokenWriter {
    fn write_frame<W: WriteContext>(&mut self, context: &mut W) -> Option<PacketNumber> {
        match DcStatelessResetTokens::new(self.tokens.as_slice()) {
            Ok(frame) => context.write_frame(&frame),
            Err(error) => {
                debug_assert!(
                    false,
                    "The dc provider produced invalid stateless reset tokens: {}",
                    error
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;
