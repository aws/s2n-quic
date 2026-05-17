// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::EndpointConfig;
use s2n_quic_dc::{
    busy_poll,
    path::secret::{self, stateless_reset::Signer},
    runtime,
    socket::rate::Rate,
    stream::endpoint::{self, socket},
};
use std::{io, net::SocketAddr, sync::Arc};
use tracing::info;

pub fn create(
    config: &EndpointConfig,
    bind_addr: SocketAddr,
    pool: &busy_poll::Pool,
    print_pipeline_dot: bool,
) -> io::Result<Arc<endpoint::Endpoint>> {
    let inspector = runtime::inspector::Handle::new(runtime::busy_poll::Handle::new(pool.clone()));

    // Create the path secret map (shared by endpoint, client PSK, server PSK)
    let signer = Signer::new(b"dc-tester");
    let clock = s2n_quic_dc::time::tokio::Clock::default();
    let subscriber = s2n_quic_dc::event::tracing::Subscriber::default();
    let map = secret::Map::new(signer, 50_000, true, clock, subscriber);

    // Create recv sockets first to determine the data port
    let recv_sockets = socket::RecvConfig::new(config.recv_io_workers, bind_addr).busy_poll()?;

    {
        use s2n_quic_dc::socket::recv::Socket as _;
        let recv_port = recv_sockets.first().unwrap().local_addr().unwrap().port();
        info!(
            recv_io_workers = config.recv_io_workers,
            recv_port, "Recv sockets bound"
        );
    }

    // Create send sockets
    let gso = endpoint::Gso::default();
    let send_sockets =
        socket::SendConfig::new(config.send_sockets, bind_addr, gso.clone()).busy_poll()?;

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

    let layout = config.layout();
    info!(?layout, "starting endpoint");

    let send_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let recv_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let acceptor_registry = s2n_quic_dc::acceptor::Registry::new();

    let endpoint_config = endpoint::Config {
        layout,
        send_pool,
        recv_pool,
        path_secret_map: map,
        gso,
        acceptor_registry,
        overall_send_rate: Rate::new(config.bandwidth),
        per_socket_send_rate: Rate::new(config.per_socket_bandwidth),
        budgets: endpoint::Budgets::default(),
        submission_shards: config.submission_shards,
    };

    let inner = endpoint::setup_endpoint(
        inspector.clone(),
        endpoint_config,
        send_sockets,
        recv_sockets,
    );
    if print_pipeline_dot {
        println!("{}", inspector.to_dot());
        eprintln!("pipeline channel bindings:");
        for binding in inspector.channel_bindings() {
            eprintln!("{binding}");
        }
    }

    Ok(Arc::new(inner))
}
