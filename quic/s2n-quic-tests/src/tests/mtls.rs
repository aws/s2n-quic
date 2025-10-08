// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use events::HandshakeStatus;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn mtls_happy_case() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));
    const LEN: usize = 1000;

    let server_subscriber = recorder::HandshakeStatus::new();
    let server_events = server_subscriber.clone();
    let client_subscriber = recorder::HandshakeStatus::new();
    let client_events = client_subscriber.clone();

    test(model.clone(), |handle| {
        let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event((
                tracing_events(true, model.max_udp_payload()),
                server_subscriber,
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((
                tracing_events(true, model.max_udp_payload()),
                client_subscriber,
            ))?
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

    let server_handshake_complete =
        server_events.any(|x| matches!(x.status, HandshakeStatus::Complete { .. }));
    let server_handshake_confirmed =
        server_events.any(|x| matches!(x.status, HandshakeStatus::Confirmed { .. }));
    let client_handshake_complete =
        client_events.any(|x| matches!(x.status, HandshakeStatus::Complete { .. }));
    let client_handshake_confirmed =
        client_events.any(|x| matches!(x.status, HandshakeStatus::Confirmed { .. }));

    // assert handshake success for both the sever and client
    assert!(server_handshake_complete);
    assert!(server_handshake_confirmed);
    assert!(client_handshake_complete);
    assert!(client_handshake_confirmed);
}

#[test]
fn mtls_auth_failure() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_subscriber = recorder::HandshakeStatus::new();
    let server_events = server_subscriber.clone();
    let client_subscriber = recorder::HandshakeStatus::new();
    let client_events = client_subscriber.clone();

    // check that server attempts to accept but rejects a connection
    let server_connection_closed = Arc::new(AtomicBool::new(false));
    let server_connection_closed_clone = server_connection_closed.clone();

    test(model.clone(), |handle| {
        let server_tls = build_server_mtls_provider(certificates::UNTRUSTED_CERT_PEM)?;
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event((
                tracing_events(true, model.max_udp_payload()),
                server_subscriber,
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            match server.accept().await {
                Some(_) => {
                    panic!("connection should not be accepted on auth failure");
                }
                None => {
                    assert!(
                        !server_connection_closed_clone.swap(true, Ordering::SeqCst),
                        "confirm that this is only called once"
                    );
                }
            }
        });

        let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((
                tracing_events(true, model.max_udp_payload()),
                client_subscriber,
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let stream_result = conn.accept_bidirectional_stream().await;

            assert!(stream_result.is_err(), "handshake should fail");
        });

        Ok(addr)
    })
    .unwrap();

    let server_handshake_complete =
        server_events.any(|x| matches!(x.status, HandshakeStatus::Complete { .. }));
    let server_handshake_confirmed =
        server_events.any(|x| matches!(x.status, HandshakeStatus::Confirmed { .. }));
    let client_handshake_complete =
        client_events.any(|x| matches!(x.status, HandshakeStatus::Complete { .. }));
    let client_handshake_confirmed =
        client_events.any(|x| matches!(x.status, HandshakeStatus::Confirmed { .. }));

    // expect server handshake to fail
    assert!(!server_handshake_complete);
    assert!(!server_handshake_confirmed);

    // expect the client handshake status to 'Complete' but not 'Confirmed'. We
    // expect server's client-certificate-authentication (mTLS) to fail, which
    // happens after client TLS handshake 'Complete'
    assert!(client_handshake_complete);
    assert!(!client_handshake_confirmed);

    // confirm server connection was attempted but failed
    assert!(server_connection_closed.load(Ordering::SeqCst));
}
