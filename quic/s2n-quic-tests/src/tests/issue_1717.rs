// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// Ensures PTO backoff is reset once per space discard
///
/// See https://github.com/aws/s2n-quic/pull/1717
#[test]
fn increasing_pto_count_under_loss() {
    let delay_time = Duration::from_millis(10);

    let model = Model::default();
    model.set_delay(delay_time);
    let subscriber = recorder::Pto::new();
    let pto_events = subscriber.events();

    test(model.clone(), |handle| {
        spawn(async move {
            // allow for 1 RTT worth of data and then drop all packet after
            // the client gets an initial ACK from the server
            delay(delay_time * 2).await;
            model.set_drop_rate(1.0);
        });

        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            if let Some(conn) = server.accept().await {
                delay(Duration::from_secs(10)).await;
                let _ = conn;
            }
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?
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

    // assert that sufficient recovery events were captured
    let pto_len = pto_events.len();
    assert!(pto_len > 10);
    // the last recovery event is fired after we discard the handshake space so ignore it
    pto_events.truncate(pto_len - 1);

    let pto_count: u32 = *pto_events
        .iter()
        .reduce(|prev, new| {
            // assert that the value is monotonically increasing
            assert!(new >= prev, "prev_value {prev}, new_value {new}");
            new
        })
        .unwrap();

    // assert that the final pto_count increased to some large value over the
    // duration of the test
    assert!(
        pto_count > 5,
        "delay: {delay_time:?}. pto_count: {pto_count}"
    );
}
