// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn optimistic_ack_mitigation() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));
    const LEN: usize = 1_000_000;

    let server_subscriber = recorder::PacketSkipped::new();
    let server_events = server_subscriber.events();
    let client_subscriber = recorder::PacketSkipped::new();
    let client_events = server_subscriber.events();
    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();

            let mut recv_len = 0;
            while let Some(chunk) = stream.receive().await.unwrap() {
                recv_len += chunk.len();
            }
            assert_eq!(LEN, recv_len);
        });

        Ok(addr)
    })
    .unwrap();

    let server_skip_count = server_events
        .lock()
        .unwrap()
        .iter()
        .filter(|reason| {
            matches!(
                reason,
                events::PacketSkipReason::OptimisticAckMitigation { .. }
            )
        })
        .count();
    let client_skip_count = client_events
        .lock()
        .unwrap()
        .iter()
        .filter(|reason| {
            matches!(
                reason,
                events::PacketSkipReason::OptimisticAckMitigation { .. }
            )
        })
        .count();

    // Verify that both client and server are skipping packets for Optimistic
    // Ack attack mitigation.
    assert_eq!(server_skip_count, 5);
    assert_eq!(client_skip_count, 5);
}
