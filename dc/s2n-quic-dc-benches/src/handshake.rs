// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark for a single dc-quic handshake. Useful for comparing different dc-quic configurations
//! settings against each other. Note that because this benchmark uses real sockets
//! the results are inherently variable, but should still be useful for relative comparisons.

use criterion::{criterion_group, criterion_main, Criterion};
use s2n_quic::{provider::tls::Provider, server::Name};
use s2n_quic_core::time::StdClock;
use s2n_quic_dc::{
    path::secret::{stateless_reset::Signer, Map},
    testing::{NoopSubscriber, TestTlsProvider},
};
use std::time::{Duration, Instant};

struct TestSetup {
    server_addr: std::net::SocketAddr,
    server_name: Name,
    tls: TestTlsProvider,
    subscriber: NoopSubscriber,
}

async fn make_server() -> (TestSetup, tokio_util::sync::DropGuard) {
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

    let server_addr = server_addr_rx.await.unwrap().unwrap();
    let server_name: s2n_quic::server::Name = "localhost".into();

    (
        TestSetup {
            server_addr,
            server_name,
            tls,
            subscriber,
        },
        drop_guard,
    )
}

fn make_client(setup: &TestSetup) -> s2n_quic_dc::psk::client::Provider {
    let client_map = Map::new(
        Signer::new(b"default"),
        50_000,
        false,
        StdClock::default(),
        setup.subscriber.clone(),
    );

    s2n_quic_dc::psk::client::Provider::builder()
        .start(
            "0.0.0.0:0".parse().unwrap(),
            client_map,
            setup.tls.clone().start_client().unwrap(),
            setup.subscriber.clone(),
            "localhost".into(),
        )
        .unwrap()
}

pub fn benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (setup, drop_guard) = runtime.block_on(make_server());

    c.bench_function("handshake", |b| {
        b.to_async(&runtime).iter_custom(|iters| {
            let setup = &setup;
            async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    // Fresh client per iteration, otherwise the handshake will be cached and reused
                    let client = make_client(setup);

                    let start = Instant::now();
                    client
                        .unconditionally_handshake_with_entry(
                            setup.server_addr,
                            setup.server_name.clone(),
                        )
                        .await
                        .unwrap();
                    total += start.elapsed();
                }
                total
            }
        })
    });

    drop(drop_guard);
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
