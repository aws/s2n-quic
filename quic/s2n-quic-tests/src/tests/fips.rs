// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

compat_test!(default_fips_test {
    use server_provider::tls::default as server_tls;
    use client_provider::tls::default as client_tls;

    let model = Model::default();

    // TODO switch this to `default_fips` when the policy supports TLS 1.3
    //      see https://github.com/aws/s2n-quic/issues/2247
    let server_policy =
        server_tls::security::Policy::from_version("20230317").unwrap();
    let client_policy =
        client_tls::security::Policy::from_version("20230317").unwrap();

    test(model.clone(), |handle| {
        let server = server_tls::Server::from_loader({
            let mut builder = server_tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(["h3"])?
                .set_security_policy(&server_policy)?
                .load_pem(
                    server_certificates::CERT_PEM.as_bytes(),
                    server_certificates::KEY_PEM.as_bytes(),
                )?;

            builder.build()?
        });

        let server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(server)?
            .with_event(server_tracing_events(true, model.clone()))?
            .with_random(ServerRandom::with_seed(456))?
            .start()?;

        let client = client_tls::Client::from_loader({
            let mut builder = client_tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(["h3"])?
                .set_security_policy(&client_policy)?
                .trust_pem(client_certificates::CERT_PEM.as_bytes())?;

            builder.build()?
        });

        let client = Client::builder()
            .with_io(client_handle(handle).builder().build()?)?
            .with_tls(client)?
            .with_event(client_tracing_events(true, model.clone()))?
            .with_random(ClientRandom::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, client_core::stream::testing::Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
});
