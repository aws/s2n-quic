// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::tls::default::{self as tls, security};

fn test_policy(policy: &security::Policy) {
    let model = Model::default();

    test(model.clone(), |handle| {
        let server = tls::Server::from_loader({
            let mut builder = tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(["h3"])?
                .set_security_policy(policy)?
                .load_pem(
                    certificates::CERT_PEM.as_bytes(),
                    certificates::KEY_PEM.as_bytes(),
                )?;

            builder.build()?
        });

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client = tls::Client::from_loader({
            let mut builder = tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(["h3"])?
                .set_security_policy(policy)?
                .trust_pem(certificates::CERT_PEM.as_bytes())?;

            builder.build()?
        });

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}

#[test]
fn default_fips_test() {
    // TODO switch this to `default_fips` when the policy supports TLS 1.3
    //      see https://github.com/aws/s2n-quic/issues/2247
    test_policy(&security::Policy::from_version("20230317").unwrap());
}
