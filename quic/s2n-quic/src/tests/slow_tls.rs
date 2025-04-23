// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[test]
#[cfg(not(feature = "provider-tls-fips"))]
fn slow_default_tls() {
    use super::*;
    use crate::provider::tls::default;
    use s2n_quic_core::crypto::tls::testing::certificates::{CERT_PEM, KEY_PEM};

    let model = Model::default();

    let server_endpoint = default::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_server = SlowTlsProvider {
        endpoint: server_endpoint,
    };

    let client_endpoint = default::Client::builder()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_client = SlowTlsProvider {
        endpoint: client_endpoint,
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
