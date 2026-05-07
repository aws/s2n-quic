// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Pre-shared key management for the wheel-demo.
//!
//! Uses the PSK builder API from s2n-quic-dc to set up handshake-based
//! client and server providers using test TLS certificates.

use s2n_quic::provider::tls::default as tls;
use s2n_quic_core::crypto::tls::testing::certificates;
use s2n_quic_dc::{
    path::secret::{self, stateless_reset::Signer},
    psk,
};
use std::{io, net::SocketAddr};

pub use psk::{client::Provider as Client, server::Provider as Server};

pub type Subscriber = s2n_quic_dc::event::tracing::Subscriber;

pub fn subscriber() -> Subscriber {
    s2n_quic_dc::event::tracing::Subscriber::default()
}

pub fn server_name() -> s2n_quic::server::Name {
    s2n_quic::server::Name::from("localhost")
}

fn map() -> secret::Map {
    let signer = Signer::new(b"wheel-demo");
    let clock = s2n_quic_dc::clock::tokio::Clock::default();
    let subscriber = subscriber();
    secret::Map::new(signer, 10_000, true, clock, subscriber)
}

fn tls_server() -> io::Result<tls::Server> {
    tls::Server::builder()
        .with_application_protocols(["dcquic"].iter())
        .map_err(io::Error::other)?
        .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
        .map_err(io::Error::other)?
        .build()
        .map_err(io::Error::other)
}

fn tls_client() -> io::Result<tls::Client> {
    tls::Client::builder()
        .with_application_protocols(["dcquic"].iter())
        .map_err(io::Error::other)?
        .with_certificate(certificates::CERT_PEM)
        .map_err(io::Error::other)?
        .build()
        .map_err(io::Error::other)
}

pub async fn server(handshake_addr: SocketAddr) -> io::Result<psk::server::Provider> {
    let map = map();
    let tls = tls_server()?;
    let subscriber = s2n_quic::provider::event::default::Subscriber::default();

    psk::server::Provider::builder()
        .start(handshake_addr, tls, subscriber, map)
        .await
        .map_err(io::Error::other)
}

pub fn client() -> io::Result<psk::client::Provider> {
    let map = map();
    let tls = tls_client()?;
    let subscriber = s2n_quic::provider::event::default::Subscriber::default();

    psk::client::Provider::builder()
        .start(
            "[::]:0".parse().unwrap(),
            map,
            tls,
            subscriber,
            server_name(),
        )
        .map_err(io::Error::other)
}

// ── PSK Provider ───────────────────────────────────────────────────────────

/// Wrapper for either client or server PSK provider
pub enum PskProvider {
    Client(s2n_quic_dc::psk::client::Provider),
    Server(s2n_quic_dc::psk::server::Provider),
}

impl PskProvider {
    /// Get the underlying path secret map
    pub fn map(&self) -> &s2n_quic_dc::path::secret::map::Map {
        match self {
            PskProvider::Client(provider) => provider.map(),
            PskProvider::Server(provider) => provider.map(),
        }
    }
}
