// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

// Ensures PTO backoff is reset once per space discard
//
// See https://github.com/aws/s2n-quic/pull/1717
compat_test!(increasing_pto_count_under_loss {
    let delay_time = Duration::from_millis(10);

    let model = Model::default();
    model.set_delay(delay_time);
    let subscriber = client_recorder::Pto::new();
    let pto_events = subscriber.events();

    test(model.clone(), |handle| {
        let model_for_spawn = model.clone();
        spawn(async move {
            delay(delay_time * 2).await;
            model_for_spawn.set_drop_rate(1.0);
        });

        let mut server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(server_tracing_events(true, model.clone()))?
            .with_random(ServerRandom::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            if let Some(conn) = server.accept().await {
                delay(Duration::from_secs(10)).await;
                let _ = conn;
            }
        });

        let client = Client::builder()
            .with_io(client_handle(handle).builder().build().unwrap())?
            .with_tls(client_certificates::CERT_PEM)?
            .with_event((client_tracing_events(true, model.clone()), subscriber))?
            .with_random(ClientRandom::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let conn = client.connect(connect).await.unwrap();

            delay(Duration::from_secs(10)).await;
            let _ = conn;
        });

        Ok(addr)
    })
    .unwrap();

    let mut pto_events = pto_events.lock().unwrap();

    let pto_len = pto_events.len();
    assert!(pto_len > 10);
    pto_events.truncate(pto_len - 1);

    let pto_count: u32 = *pto_events
        .iter()
        .reduce(|prev, new| {
            assert!(new >= prev, "prev_value {prev}, new_value {new}");
            new
        })
        .unwrap();

    assert!(
        pto_count > 5,
        "delay: {delay_time:?}. pto_count: {pto_count}"
    );
});
