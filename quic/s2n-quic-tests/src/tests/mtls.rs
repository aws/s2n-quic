// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

compat_test!(mtls_happy_case {
    use server_core::event::api::HandshakeStatus as ServerHandshakeStatus;
    use client_core::event::api::HandshakeStatus as ClientHandshakeStatus;

    let model = Model::default();
    model.set_delay(Duration::from_millis(50));
    const LEN: usize = 1000;

    let server_sub = server_recorder::HandshakeStatus::new();
    let server_events = server_sub.clone();
    let client_sub = client_recorder::HandshakeStatus::new();
    let client_events = client_sub.clone();

    test(model.clone(), |handle| {
        let server_tls = server_build_mtls_provider(server_certificates::MTLS_CA_CERT)?;
        let mut server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(server_tls)?
            .with_event((server_tracing_events(true, model.clone()), server_sub))?
            .with_random(ServerRandom::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let client_tls = client_build_mtls_provider(client_certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(client_handle(handle).builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((client_tracing_events(true, model.clone()), client_sub))?
            .with_random(ClientRandom::with_seed(456))?
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

    // assert handshake success for both the server and client
    assert!(server_events.any(|x| matches!(x.status, ServerHandshakeStatus::Complete { .. })));
    assert!(server_events.any(|x| matches!(x.status, ServerHandshakeStatus::Confirmed { .. })));
    assert!(client_events.any(|x| matches!(x.status, ClientHandshakeStatus::Complete { .. })));
    assert!(client_events.any(|x| matches!(x.status, ClientHandshakeStatus::Confirmed { .. })));
});

compat_test!(mtls_auth_failure {
    use server_core::event::api::HandshakeStatus as ServerHandshakeStatus;
    use client_core::event::api::HandshakeStatus as ClientHandshakeStatus;

    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_sub = server_recorder::HandshakeStatus::new();
    let server_events = server_sub.clone();
    let client_sub = client_recorder::HandshakeStatus::new();
    let client_events = client_sub.clone();

    let server_connection_closed = Arc::new(AtomicBool::new(false));
    let server_connection_closed_clone = server_connection_closed.clone();

    test(model.clone(), |handle| {
        let server_tls = server_build_mtls_provider(server_certificates::UNTRUSTED_CERT_PEM)?;
        let mut server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(server_tls)?
            .with_event((server_tracing_events(true, model.clone()), server_sub))?
            .with_random(ServerRandom::with_seed(456))?
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

        let client_tls = client_build_mtls_provider(client_certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(client_handle(handle).builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((client_tracing_events(true, model.clone()), client_sub))?
            .with_random(ClientRandom::with_seed(456))?
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

    // expect server handshake to fail
    assert!(!server_events.any(|x| matches!(x.status, ServerHandshakeStatus::Complete { .. })));
    assert!(!server_events.any(|x| matches!(x.status, ServerHandshakeStatus::Confirmed { .. })));

    // expect the client handshake status to 'Complete' but not 'Confirmed'. We
    // expect server's client-certificate-authentication (mTLS) to fail, which
    // happens after client TLS handshake 'Complete'
    assert!(client_events.any(|x| matches!(x.status, ClientHandshakeStatus::Complete { .. })));
    assert!(!client_events.any(|x| matches!(x.status, ClientHandshakeStatus::Confirmed { .. })));

    // confirm server connection was attempted but failed
    assert!(server_connection_closed.load(Ordering::SeqCst));
});
