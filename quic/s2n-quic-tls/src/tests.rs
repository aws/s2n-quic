// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{client, server};
use s2n_quic_core::crypto::tls::{
    self,
    testing::certificates::{CERT_PEM, KEY_PEM},
    Endpoint,
};

fn s2n_client() -> client::Client {
    client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_server() -> server::Server {
    server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn rustls_client_s2n_server_test() {
    let mut client_endpoint = s2n_quic_rustls::client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_rustls_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_quic_rustls::server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();

    run(&mut server_endpoint, &mut client_endpoint);
}

/// Executes the handshake to completion
fn run<S: Endpoint, C: Endpoint>(server: &mut S, client: &mut C) {
    let mut pair = tls::testing::Pair::new(server, client, "localhost".into());

    while pair.is_handshaking() {
        pair.poll().unwrap();
    }

    pair.finish();
}
