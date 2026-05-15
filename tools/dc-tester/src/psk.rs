// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::tls::default as tls;
use s2n_quic_core::crypto::tls::testing::certificates;
use s2n_quic_dc::{path::secret, psk};
use std::{io, net::SocketAddr};

pub fn server_name() -> s2n_quic::server::Name {
    s2n_quic::server::Name::from("localhost")
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

pub async fn server(
    handshake_addr: SocketAddr,
    data_addrs: Vec<SocketAddr>,
    map: secret::Map,
) -> io::Result<psk::server::Provider> {
    let tls = tls_server()?;
    let subscriber = s2n_quic::provider::event::default::Subscriber::default();

    psk::server::Provider::builder()
        .with_data_addrs(data_addrs)
        .start(handshake_addr, tls, subscriber, map)
        .await
        .map_err(io::Error::other)
}

pub fn client(data_addrs: Vec<SocketAddr>, map: secret::Map) -> io::Result<psk::client::Provider> {
    let tls = tls_client()?;
    let subscriber = s2n_quic::provider::event::default::Subscriber::default();

    psk::client::Provider::builder()
        .with_data_addrs(data_addrs)
        .start(
            "[::]:0".parse().unwrap(),
            map,
            tls,
            subscriber,
            server_name(),
        )
        .map_err(io::Error::other)
}
