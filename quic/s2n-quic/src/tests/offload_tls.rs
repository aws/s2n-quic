// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[test]
#[cfg(feature = "unstable-offload-tls")]
fn offload_tls() {
    use super::*;
    use crate::provider::tls::default;
    use crate::provider::tls::offload::Offload;
    use s2n_quic_core::crypto::tls::testing::certificates::{CERT_PEM, KEY_PEM};

    let model = Model::default();

    let server_endpoint = default::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();
    let client_endpoint = default::Client::builder()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let server_endpoint = Offload(server_endpoint);
    let client_endpoint = Offload(client_endpoint);
    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events())?
            .with_tls(server_endpoint)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_endpoint)?
            .with_event(tracing_events())?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}
