// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::EndpointConfig;
use s2n_quic_dc::{
    busy_poll,
    path::secret::{self, stateless_reset::Signer},
    stream2::endpoint,
};
use std::{io, net::SocketAddr, sync::Arc};
use tracing::info;

pub fn create(
    config: &EndpointConfig,
    bind_addr: SocketAddr,
    spawner: &busy_poll::Pool,
) -> io::Result<Arc<endpoint::Endpoint>> {
    // Create the path secret map (shared by endpoint, client PSK, server PSK)
    let signer = Signer::new(b"dc-tester");
    let clock = s2n_quic_dc::clock::tokio::Clock::default();
    let subscriber = s2n_quic_dc::event::tracing::Subscriber::default();
    let map = secret::Map::new(signer, 50_000, true, clock, subscriber);

    // Create recv sockets first to determine the data port
    let num_recv_sockets = s2n_quic_dc::stream2::Spawner::worker_count(spawner)
        .saturating_sub(1)
        .max(1);
    let recv_sockets = endpoint::create_recv_sockets(num_recv_sockets, bind_addr)?;

    {
        use s2n_quic_dc::socket::recv::Socket as _;
        let recv_port = recv_sockets.first().unwrap().local_addr().unwrap().port();
        info!(num_recv_sockets, recv_port, "Recv sockets bound");
    }

    // Create send sockets
    let gso = endpoint::Gso::default();
    let send_sockets = endpoint::create_send_sockets(config.send_sockets, bind_addr, gso.clone())?;

    {
        use s2n_quic_dc::socket::send::Socket as _;
        let send_ports: Vec<u16> = send_sockets
            .iter()
            .map(|s| s.local_addr().unwrap().port())
            .collect();
        info!(
            num_send_sockets = config.send_sockets,
            ?send_ports,
            "Send sockets created"
        );
    }

    // Build endpoint config
    let bp_clock =
        s2n_quic_dc::busy_poll::clock::Timer::new(s2n_quic_dc::clock::tokio::Clock::default());
    let send_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let recv_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let counters = endpoint::CounterRegistry::new();
    let acceptor_registry = s2n_quic_dc::acceptor::Registry::new();

    let endpoint_config = endpoint::EndpointConfig {
        overall_send_rate: s2n_quic_dc::socket::rate::Rate::new(config.bandwidth),
        per_socket_send_rate: s2n_quic_dc::socket::rate::Rate::new(config.per_socket_bandwidth),
        spawner,
        clock: bp_clock,
        send_pool,
        recv_pool,
        counters,
        path_secret_map: map,
        gso,
        acceptor_registry,
        verbose_socket_metrics: config.verbose_socket_metrics,
    };

    let inner = endpoint::setup_endpoint(endpoint_config, send_sockets, recv_sockets, || {
        s2n_quic_dc::random::Random::default()
    });

    Ok(Arc::new(inner))
}
