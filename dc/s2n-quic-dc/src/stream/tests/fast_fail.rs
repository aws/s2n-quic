// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::{stateless_reset::Signer, Map},
    psk::client::Provider as ClientProvider,
    stream::{
        client::{error as client_error, tokio::Client as ClientTokio},
        testing::Server,
        Protocol,
    },
    testing::{init_tracing, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;

fn build_client(fail_fast: bool) -> (ClientTokio<ClientProvider, NoopSubscriber>, Map) {
    let tls_materials_provider = TestTlsProvider {};
    let subscriber = NoopSubscriber {};

    let client_map = Map::new(
        Signer::new(b"default"),
        100,
        false,
        StdClock::default(),
        subscriber.clone(),
    );

    let handshake_client = ClientProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            client_map.clone(),
            tls_materials_provider,
            subscriber.clone(),
            server_name(),
        )
        .unwrap();

    let stream_client = ClientTokio::<ClientProvider, NoopSubscriber>::builder()
        .with_tcp(true)
        .with_default_protocol(Protocol::Tcp)
        .with_fail_fast_on_missing_psk(fail_fast)
        .build(handshake_client, subscriber)
        .unwrap();

    (stream_client, client_map)
}

fn is_fast_fail(err: &std::io::Error) -> bool {
    matches!(
        client_error::Kind::from_io(err),
        Some(client_error::Kind::PeerPskMissing)
    )
}

/// Flag enabled + no cached PSK: connect must fail immediately rather than block on a handshake,
/// and the `io::Error` must carry the typed `PeerPskMissing` reason (surfaced as `WouldBlock`).
#[tokio::test]
async fn connect_fails_fast_when_psk_missing() {
    init_tracing();

    let server = Server::tcp().build();
    let (client, _client_map) = build_client(true);
    let addr = server.local_addr();
    let client_addr = "127.0.0.1:1337".parse().unwrap();

    let err = client
        .connect(client_addr, addr, server_name())
        .await
        .expect_err("connect should fail fast when no PSK is cached");

    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    assert!(
        is_fast_fail(&err),
        "expected a recoverable PeerPskMissing connect error, got: {err:?}",
    );
}

/// Flag enabled + PSK cached: connect must take the cached-secret path and succeed.
#[tokio::test]
async fn connect_succeeds_when_psk_cached() {
    init_tracing();

    let server = Server::tcp().build();
    let (client, client_map) = build_client(true);
    let addr = server.local_addr();
    let client_addr = "127.0.0.1:1337".parse().unwrap();

    let _ = client_map.test_insert_pair(client_addr, None, server.map(), addr, None);

    let result = tokio::try_join!(client.connect(addr, addr, server_name()), server.accept());
    assert!(
        result.is_ok(),
        "connect should succeed once a PSK is cached, got: {:?}",
        result.err(),
    );
}
