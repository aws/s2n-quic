// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::EndpointConfig;
use s2n_quic_dc::{
    busy_poll,
    path::secret::{self, stateless_reset::Signer},
    runtime,
    socket::{rate::Rate, LocalAddr},
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
    let runtime = runtime::busy_poll::Handle::new(pool.clone());

    // Create the path secret map (shared by endpoint, client PSK, server PSK)
    let signer = Signer::new(b"dc-tester");
    let clock = s2n_quic_dc::time::tokio::Clock::default();
    let subscriber = s2n_quic_dc::event::tracing::Subscriber::default();
    let map = secret::Map::new(signer, 50_000, true, clock, subscriber);

    let gso = endpoint::Gso::default();
    let num_sockets = config.recv_io_workers.max(config.send_sockets);
    let bind_addrs = (0..num_sockets)
        .map(|_| {
            let mut addr = bind_addr;
            addr.set_port(0);
            addr.into()
        })
        .collect();
    let socket_config = socket::Config::new(
        bind_addrs,
        config.send_sockets,
        config.recv_io_workers,
        gso.clone(),
    );
    let (send_sockets, recv_sockets) = socket_config.busy_poll()?;

    {
        let recv_port = recv_sockets.first().unwrap().local_addr().unwrap().port();
        info!(
            recv_io_workers = config.recv_io_workers,
            recv_port, "Recv sockets bound"
        );
    }

    {
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

    // Create a single UPS (UnknownPathSecret) send socket bound to an ephemeral port.
    let ups_socket = {
        let mut opts = s2n_quic_dc::socket::Options::default();
        opts.addr = bind_addr;
        opts.addr.set_port(0);
        opts.blocking = false;
        opts.send_buffer = Some(1 << 20); // 1 MiB
        opts.recv_buffer = Some(0);
        let socket = opts.build_udp()?;
        s2n_quic_dc::socket::Gso(s2n_quic_dc::socket::BusyPoll(socket), gso.clone())
    };

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
        ups_rate: Rate::new(0.001), // 1 Mbps — small budget; UPS is low-volume control traffic
        ups_dedup_capacity: 1024,
        ups_dedup_window: core::time::Duration::from_secs(1),
        dead_peer_cooldown: core::time::Duration::from_secs(5),
        initial_tx_descriptor_allocs: 2,
        initial_rx_descriptor_allocs: 64,
        // Send pool: bounds local pre-transmission frame queuing; credit releases at admission to
        // the inflight map (~10us), not at ACK, so it can be far smaller than the recv pool.
        // `max_single_acquire` MUST stay well above one msg chunk (`msg_packet_size`, ~8.8 KiB at
        // the production 8940-byte MTU). A writer acquires per chunk and the QueueMsg resume path
        // takes a full chunk at a time; if a single acquire can't cover one chunk the writer
        // either stalls (it never advertises enough for forward progress) or, on the resume path,
        // truncates a chunk's `flow_credits` and corrupts receiver flow accounting. The default
        // `Config::new` cap (capacity/256) would be only 8 KiB at a 2 MiB capacity — below a chunk
        // — so set a uniform cap that comfortably spans several chunks.
        // TODO(measure): tune capacity to the empirical acquire->admit latency via dc-tester.
        send_credit_pool_config: s2n_quic_dc::credit::Config::new(2 * 1024 * 1024)
            .with_max_single_acquire_uniform(256 * 1024),
        // Recv pool: aggregate advertised-but-unfilled receive window across all streams. Sized to
        // ~8x the single-stream BDP (30 Gbps x 500us ~= 1.875 MB) so several streams can hold a
        // full window concurrently. `max_single_acquire` is the per-stream window ceiling — set to
        // ~1 BDP (2 MiB) so a single stream can saturate the link but no further. A reader extends
        // by a full window per acquire, so this must stay >= the per-stream window.
        recv_credit_pool_config: s2n_quic_dc::credit::Config::new(16 * 1024 * 1024)
            .with_max_single_acquire_uniform(2 * 1024 * 1024),
    };

    let inner = endpoint::setup_endpoint(
        runtime,
        endpoint_config,
        send_sockets,
        recv_sockets,
        ups_socket,
    );
    if print_pipeline_dot {
        let topology = inner.counters.topology();
        println!("{}", topology.to_dot());
        eprintln!("pipeline channel bindings:");
        for binding in topology.bindings {
            eprintln!(
                "task '{}' {} channel '{}' ({}, fn: {})",
                binding.task_name,
                binding.direction,
                binding.channel_name,
                binding.description,
                binding.function
            );
        }
    }

    Ok(Arc::new(inner))
}
