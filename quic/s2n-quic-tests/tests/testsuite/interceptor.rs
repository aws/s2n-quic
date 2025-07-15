// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn intercept_loss(loss: Loss<Random>) {
    let model = Model::default();
    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .with_packet_interceptor(loss)?
            .start()?;
        let server_address = start_server(server)?;

        client(handle, server_address)
    })
    .unwrap();
}

#[test]
fn interceptor_success_test() {
    intercept_loss(
        Loss::builder(Random::with_seed(123))
            .with_rx_loss(0..20)
            .with_rx_pass(1..5)
            .build(),
    )
}

#[test]
#[should_panic]
fn interceptor_failure_test() {
    intercept_loss(
        Loss::builder(Random::with_seed(123))
            .with_rx_loss(0..20)
            .with_rx_pass(1..2)
            .build(),
    )
}
