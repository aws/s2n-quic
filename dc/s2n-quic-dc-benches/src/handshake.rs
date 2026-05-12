// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark for a single dc-quic handshake. Useful for comparing different dc-quic configurations
//! settings against each other. Note that because this benchmark uses real sockets
//! the results are inherently variable, but should still be useful for relative comparisons.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use s2n_quic::{provider::tls::Provider, server::Name};
use s2n_quic_core::time::StdClock;
use s2n_quic_dc::{
    path::secret::{stateless_reset::Signer, Map},
    testing::{NoopSubscriber, TestTlsProvider},
};

struct TestSetup {
    client: s2n_quic_dc::psk::client::Provider,
    server_addr: std::net::SocketAddr,
    server_name: Name,
}

async fn setup() -> (TestSetup, tokio_util::sync::DropGuard) {
    let server_builder = s2n_quic_dc::psk::server::Builder::default();

    let tls = TestTlsProvider {};
    let subscriber = NoopSubscriber {};

    let server_map = Map::new(
        Signer::new(b"default"),
        50_000,
        false,
        StdClock::default(),
        subscriber.clone(),
    );

    let (server_addr_rx, drop_guard) = s2n_quic_dc::psk::server::Provider::setup(
        "127.0.0.1:0".parse().unwrap(),
        server_map.clone(),
        tls.clone(),
        subscriber.clone(),
        server_builder,
    );

    let client_map = Map::new(
        Signer::new(b"default"),
        50_000,
        false,
        StdClock::default(),
        subscriber.clone(),
    );

    let client = s2n_quic_dc::psk::client::Provider::builder()
        .start(
            "0.0.0.0:0".parse().unwrap(),
            client_map,
            tls.start_client().unwrap(),
            subscriber,
            "localhost".into(),
        )
        .unwrap();

    let server_addr = server_addr_rx.await.unwrap().unwrap();
    let server_name: s2n_quic::server::Name = "localhost".into();

    (
        TestSetup {
            client,
            server_addr,
            server_name,
        },
        drop_guard,
    )
}

pub fn benchmarks(c: &mut Criterion) {
    c.bench_function("dc-quic", move |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_batched(
                || futures::executor::block_on(setup()),
                |(setup, drop_guard)| async move {
                    setup
                        .client
                        .unconditionally_handshake_with_entry(setup.server_addr, setup.server_name)
                        .await
                        .unwrap();
                    drop(drop_guard)
                },
                BatchSize::SmallInput,
            )
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
