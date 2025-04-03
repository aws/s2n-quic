// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "s2n-quic-tls")]
#[test]
fn slow_tls() {
    use super::*;
    use crate::provider::tls::s2n_tls;
    use s2n_quic_core::crypto::tls::testing::certificates::{CERT_PEM, KEY_PEM};

    let model = Model::default();

    let server_endpoint = s2n_tls::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_server = SlowTlsProvider {
        server_endpoint: Some(server_endpoint),
        client_endpoint: None,
    };

    let client_endpoint = s2n_tls::Client::builder()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_client = SlowTlsProvider {
        client_endpoint: Some(client_endpoint),
        server_endpoint: None,
    };

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(slow_server)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(slow_client)?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

#[cfg(feature = "s2n-quic-rustls")]
#[test]
fn slow_rustls() {
    use super::*;
    use crate::{provider::tls::rustls, tests::SlowTlsProvider};
    use s2n_quic_core::crypto::tls::testing::certificates::{CERT_PEM, KEY_PEM};

    let model = Model::default();

    let server_endpoint = rustls::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_server = SlowTlsProvider {
        server_endpoint: Some(server_endpoint),
        client_endpoint: None,
    };

    let client_endpoint = rustls::Client::builder()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_client = SlowTlsProvider {
        client_endpoint: Some(client_endpoint),
        server_endpoint: None,
    };

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(slow_server)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(slow_client)?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}
