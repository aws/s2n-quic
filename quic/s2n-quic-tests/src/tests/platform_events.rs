// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::event::testing::endpoint;

#[test]
fn platform_events() {
    let model = Model::default();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls((certificates::CERT_PEM, certificates::KEY_PEM))?
            .with_event((
                tracing_events(true),
                endpoint::Subscriber::named_snapshot("platform_events__server"),
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event((
                tracing_events(true),
                endpoint::Subscriber::named_snapshot("platform_events__client"),
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}
