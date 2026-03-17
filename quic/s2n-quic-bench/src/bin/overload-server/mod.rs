// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This benchmark runs a single s2n-quic server and then attempts to overload it by running N
//! concurrent handshakes. As of writing those clients are using s2n-quic-dc which means they're
//! deduplicated, but it would be good to use a regular s2n-quic client if possible in the long
//! run.
//!
//! Metrics are printed to stdout every second.

use clap::Parser;
use s2n_quic_core::{crypto::tls::testing::certificates, time::StdClock};
use s2n_quic_dc::{
    path::secret::{self, stateless_reset},
    psk::{client, server},
};
use s2n_quic_dc_metrics::{Registry, Unit};
use std::{
    net::Ipv4Addr,
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[global_allocator]
static A: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    concurrency: usize,
}

fn new_map(capacity: usize) -> secret::Map {
    secret::Map::new(
        stateless_reset::Signer::new(b"bench"),
        capacity,
        false,
        StdClock::default(),
        s2n_quic_dc::event::disabled::Subscriber::default(),
    )
}

/// Metrics backed by s2n-quic-dc-metrics Registry for formatted display.
#[derive(Clone)]
struct Metrics {
    success: s2n_quic_dc_metrics::BoolCounter,
    latency: s2n_quic_dc_metrics::Summary,
}

impl Metrics {
    fn new(registry: &Registry) -> Self {
        Self {
            success: registry.register_bool("success".into(), None),
            latency: registry.register_summary("latency".into(), None, Unit::Microsecond),
        }
    }

    fn record(&self, ok: bool, elapsed: Duration) {
        self.success.record(ok);
        self.latency.record_duration(elapsed);
    }
}

#[cfg(not(target_os = "windows"))]
mod mtls {
    use super::*;
    use s2n_quic::provider::tls;

    type Error = Box<dyn std::error::Error + Send + Sync>;

    pub fn build_client_mtls_provider(ca_cert: &str) -> Result<tls::default::Client, Error> {
        let tls = tls::default::Client::builder()
            .with_certificate(ca_cert)?
            .with_client_identity(
                certificates::MTLS_CLIENT_CERT,
                certificates::MTLS_CLIENT_KEY,
            )?
            .build()?;
        Ok(tls)
    }

    pub fn build_server_mtls_provider(ca_cert: &str) -> Result<tls::default::Server, Error> {
        let tls = tls::default::Server::builder()
            .with_certificate(
                certificates::MTLS_SERVER_CERT,
                certificates::MTLS_SERVER_KEY,
            )?
            .with_client_authentication()?
            .with_trusted_certificate(ca_cert)?
            .build()?;
        Ok(tls)
    }
}

pub fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_env("S2N_LOG"))
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();
    tracing::info!("Running...");

    let Args { concurrency } = Args::parse();
    println!("Starting benchmark with {concurrency} clients");

    let sub = s2n_quic::provider::event::disabled::Subscriber;

    let server = server::Provider::builder()
        .start_blocking(
            "127.0.0.1:0".parse().unwrap(),
            mtls::build_server_mtls_provider(certificates::MTLS_CA_CERT)?,
            sub,
            new_map(500),
        )
        .unwrap();

    let registry = Registry::new();
    let metrics = Metrics::new(&registry);

    let redirector = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name_fn(|| String::from("proxy"))
            .worker_threads(8)
            .build()
            .unwrap(),
    );

    std::thread::spawn({
        let r = redirector.clone();
        move || {
            r.block_on(std::future::pending::<()>());
        }
    });

    let server_name: s2n_quic::server::Name = "localhost".into();

    let num_groups = concurrency.div_ceil(5);
    let barrier = Arc::new(Barrier::new(num_groups + 1));

    // A redirect socket sits between a client <-> redirect <-> server.
    //
    // Clients can only handshake with up to 5 *distinct* server addresses, so having the redirect
    // in the middle allows us to treat the single server as many different servers. Each client we
    // spin up gets a set of 5 redirect sockets which we read/write from to get to the actual
    // server.
    for _ in 0..num_groups {
        let sub = s2n_quic::provider::event::disabled::Subscriber;

        let client = client::Provider::builder()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_await_dedup_removal(true)
            .start(
                "127.0.0.1:0".parse().unwrap(),
                new_map(10),
                mtls::build_client_mtls_provider(certificates::MTLS_CA_CERT).unwrap(),
                sub,
                server_name.clone(),
            )?;

        let client_addr = client.local_addr()?;
        let server_addr = server.local_addr();

        // These will redirect to the single shared server.
        let addrs = (0..5)
            .map(|_| {
                let r = redirector.clone();
                redirector.block_on(async move {
                    let socket = tokio::net::UdpSocket::bind(std::net::SocketAddrV4::new(
                        Ipv4Addr::LOCALHOST,
                        0,
                    ))
                    .await
                    .unwrap();

                    let socket_addr = socket.local_addr().unwrap();

                    // Increase receive buffer to avoid packet drops under load.
                    // See nstat -a -r | grep 'UdpMemErrors'.
                    nix::sys::socket::setsockopt(
                        &socket,
                        nix::sys::socket::sockopt::RcvBuf,
                        &(512 * 1024),
                    )
                    .unwrap();

                    r.spawn(async move {
                        let mut packet = vec![0; 10_000];
                        loop {
                            let (len, src) = socket.recv_from(&mut packet[..]).await.unwrap();
                            if src == server_addr {
                                socket.send_to(&packet[..len], client_addr).await.unwrap();
                            } else if src == client_addr {
                                socket.send_to(&packet[..len], server_addr).await.unwrap();
                            } else {
                                unreachable!("unknown packet src: {:?}", src);
                            }
                        }
                    });

                    socket_addr
                })
            })
            .collect::<Vec<_>>();

        let metrics = metrics.clone();
        let server_name = server_name.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut futs = tokio::task::JoinSet::new();
                let mut first = true;
                loop {
                    let start = Instant::now();
                    for addr in addrs.iter().copied() {
                        let client = client.clone();
                        let sn = server_name.clone();
                        futs.spawn(async move {
                            client.unconditionally_handshake_with_entry(addr, sn).await
                        });
                    }
                    while let Some(Ok(res)) = futs.join_next().await {
                        metrics.record(res.is_ok(), start.elapsed());
                    }
                    if first {
                        first = false;
                        barrier.wait();
                    }
                }
            });
        });
    }

    barrier.wait();

    // Skip the first metrics line (warmup)
    std::thread::sleep(Duration::from_secs(1));
    let _ = registry.take_current_metrics_line();

    loop {
        std::thread::sleep(Duration::from_secs(1));
        let line = registry.take_current_metrics_line();
        println!("{line}");
    }
}
