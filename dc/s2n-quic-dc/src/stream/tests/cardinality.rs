// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::{stateless_reset::Signer, Map},
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        client::tokio::Client as ClientTokio, server::tokio::Server as ServerTokio, Protocol,
    },
    testing::{init_tracing, query_event, NoopSubscriber, TestTlsProvider, SNI},
};
use s2n_quic_core::time::StdClock;
use std::{collections::HashSet, net::SocketAddr, num::NonZeroUsize, time::Duration};

#[tokio::test]
async fn many_servers() {
    init_tracing();

    let tls_materials_provider = TestTlsProvider {};
    let test_event_subscriber = NoopSubscriber {};
    let target = 5000;

    let client = ClientProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            Map::new(
                Signer::new(b"default"),
                target * 5,
                StdClock::default(),
                test_event_subscriber.clone(),
            ),
            tls_materials_provider.clone(),
            test_event_subscriber.clone(),
            query_event,
            SNI.to_string(),
        )
        .unwrap();

    let client = ClientTokio::<ClientProvider, NoopSubscriber>::builder()
        .with_tcp(true)
        .with_default_protocol(Protocol::Tcp)
        .build(client, test_event_subscriber.clone())
        .unwrap();

    let mut servers: Vec<(SocketAddr, Map)> = vec![];
    let mut used = HashSet::new();

    for _ in 0..target {
        let map = Map::new(
            Signer::new(b"default"),
            1,
            StdClock::default(),
            test_event_subscriber.clone(),
        );
        // we can't keep thousands of handshake::Servers around as each has a 1MB cuckoo filter.
        // Mid-to-long-term it's likely that we'll want some work on memory reduction, but this
        // works for now.
        let hs = loop {
            let hs = ServerProvider::builder()
                .start(
                    "127.0.0.1:0".parse().unwrap(),
                    tls_materials_provider.clone(),
                    test_event_subscriber.clone(),
                    map.clone(),
                )
                .await
                .unwrap();

            // only unique addresses allowed
            if used.insert(hs.local_addr()) {
                break hs;
            }

            // wait a bit to rebind to a find port
            tokio::time::sleep(Duration::from_millis(10)).await;
        };

        client.handshake_with(hs.local_addr()).await.unwrap();

        servers.push((hs.local_addr(), map));
    }

    // Now confirm we can connect to them **without** handshaking.
    for (hs_addr, map) in servers.into_iter() {
        let server = ServerTokio::<ServerProvider, NoopSubscriber>::builder()
            .with_address("127.0.0.1:0".parse().unwrap())
            .with_protocol(Protocol::Tcp)
            .with_udp(false)
            .with_workers(NonZeroUsize::new(1).unwrap())
            .build(
                ServerProvider::builder()
                    .start(
                        "127.0.0.1:0".parse().unwrap(),
                        tls_materials_provider.clone(),
                        test_event_subscriber.clone(),
                        map,
                    )
                    .await
                    .unwrap(),
                test_event_subscriber.clone(),
            )
            .unwrap();

        let acceptor_addr = server.acceptor_addr().unwrap();
        // if both are OK we successfully wrote the prelude and it was accepted, so no handshake
        // was needed.
        tokio::try_join!(client.connect(hs_addr, acceptor_addr), async {
            // if we hit an error accepting the stream, this would otherwise just hang
            // indefinitely -- errors aren't propagated up to accept().
            let x = tokio::time::timeout(Duration::from_secs(5), server.accept())
                .await
                .unwrap();
            x
        })
        .unwrap();
    }
}

#[tokio::test]
async fn many_clients() {
    init_tracing();

    let tls_materials_provider = TestTlsProvider {};
    let test_event_subscriber = NoopSubscriber {};
    let target = 5000;

    let hs = ServerProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            tls_materials_provider.clone(),
            test_event_subscriber.clone(),
            Map::new(
                Signer::new(b"default"),
                target * 5,
                StdClock::default(),
                test_event_subscriber.clone(),
            ),
        )
        .await
        .unwrap();

    let server = ServerTokio::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .build(hs, test_event_subscriber.clone())
        .unwrap();

    let mut clients: Vec<Map> = vec![];
    for _id in 0..target {
        let map = Map::new(
            Signer::new(b"default"),
            1,
            StdClock::default(),
            test_event_subscriber.clone(),
        );
        let client = ClientProvider::builder()
            .start(
                "127.0.0.1:0".parse().unwrap(),
                map.clone(),
                tls_materials_provider.clone(),
                test_event_subscriber.clone(),
                query_event,
                SNI.to_string(),
            )
            .unwrap();
        client
            .handshake_with(
                server.handshake_addr().unwrap(),
                query_event,
                SNI.to_string(),
            )
            .await
            .unwrap();
        clients.push(map);
    }

    // Now confirm we can connect **without** handshaking.
    for client in clients.iter() {
        let client = ClientProvider::builder()
            .start(
                "127.0.0.1:0".parse().unwrap(),
                client.clone(),
                tls_materials_provider.clone(),
                test_event_subscriber.clone(),
                query_event,
                SNI.to_string(),
            )
            .unwrap();
        let client = ClientTokio::<ClientProvider, NoopSubscriber>::builder()
            .with_tcp(true)
            .with_default_protocol(Protocol::Tcp)
            .build(client, test_event_subscriber.clone())
            .unwrap();
        let client_conn = tokio::net::TcpStream::connect(server.acceptor_addr().unwrap())
            .await
            .unwrap();

        // if both are OK we successfully wrote the prelude and it was accepted, so no handshake
        // was needed.
        tokio::try_join!(
            async {
                let x = client
                    .connect_tcp_with(server.handshake_addr().unwrap(), client_conn)
                    .await;
                x
            },
            async {
                // if we hit an error accepting the stream, this would otherwise just hang
                // indefinitely -- errors aren't propagated up to accept().
                let x = tokio::time::timeout(Duration::from_secs(5), server.accept())
                    .await
                    .unwrap();
                x
            }
        )
        .unwrap();
    }
}
