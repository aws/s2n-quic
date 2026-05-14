// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::testing::dcquic::tcp::*;
use crate::{
    path::secret::{stateless_reset::Signer, Map},
    psk::{client, server},
    stream::{socket::Protocol, testing::bind_pair},
    testing::{init_tracing, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;
use std::sync::Arc;

#[tokio::test]
async fn context_accessible() {
    let context = Context::new().await;
    let (mut client, _server) = context.pair().await;

    // Confirms that the () context on NoopSubscriber is accessible.
    client.query_event_context(|_: &()| ()).unwrap();
    let (read, write) = client.split();
    read.query_event_context(|_: &()| ()).unwrap();
    write.query_event_context(|_: &()| ()).unwrap();
}

#[tokio::test]
async fn path_application_data_none_by_default() {
    let context = Context::new().await;
    let (_client, server) = context.pair().await;

    // Without a registered make_application_data callback, the server stream
    // should have no application_data.
    assert!(server.path_application_data().is_none());
}

#[tokio::test]
async fn path_application_data_available_on_server_stream() {
    init_tracing();

    let tls = TestTlsProvider {};
    let sub = NoopSubscriber {};

    // Build server Map with application_data callback
    let server_map = Map::new(
        Signer::new(b"default"),
        50_000,
        false,
        StdClock::default(),
        sub.clone(),
    );
    server_map.register_make_application_data(Box::new(|_session| {
        let data: Arc<dyn std::any::Any + Send + Sync> = Arc::new(42u64);
        Ok(Some(data))
    }));

    let server_prov = server::Provider::builder()
        .start(
            "[::1]:0".parse().unwrap(),
            tls.clone(),
            sub.clone(),
            server_map,
        )
        .await
        .unwrap();

    let client_map = Map::new(
        Signer::new(b"default"),
        50_000,
        false,
        StdClock::default(),
        sub.clone(),
    );
    let client_prov = client::Provider::builder()
        .with_success_jitter(std::time::Duration::ZERO)
        .start(
            "[::]:0".parse().unwrap(),
            client_map,
            tls,
            sub.clone(),
            server_name(),
        )
        .unwrap();

    let (client, server) = bind_pair(
        Protocol::Tcp,
        "127.0.0.1:0".parse().unwrap(),
        client_prov,
        server_prov,
    );

    let acceptor_addr = server.acceptor_addr().expect("acceptor_addr");
    let handshake_addr = server.handshake_addr().expect("handshake_addr");

    let (client_stream, server_stream) = tokio::join!(
        async {
            client
                .connect(handshake_addr, acceptor_addr, server_name())
                .await
                .unwrap()
        },
        async { server.accept().await.unwrap().0 }
    );

    // Client doesn't have application_data
    assert!(client_stream.path_application_data().is_none());

    // Server stream should have the application_data we registered
    let app_data = server_stream
        .path_application_data()
        .expect("should have application_data");
    let value = app_data
        .downcast_ref::<u64>()
        .expect("should downcast to u64");
    assert_eq!(*value, 42u64);
}
