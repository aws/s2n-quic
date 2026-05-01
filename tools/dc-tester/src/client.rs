// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ClientConfig, WorkloadConfig};
use s2n_quic_core::{buffer::Reader as _, stream::testing::Data};
use s2n_quic_dc::{psk, stream::socket};
use std::{io, net::SocketAddr, path::PathBuf, time::Duration};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

type Subscriber = crate::psk::Subscriber;
type Client = s2n_quic_dc::stream::client::tokio::Client<psk::client::Provider, Subscriber>;

pub async fn run(
    config: ClientConfig,
    acceptor_addr: SocketAddr,
    handshake_addr: SocketAddr,
    trace_dir: &PathBuf,
) -> io::Result<()> {
    info!(
        workload_count = config.workloads.len(),
        %acceptor_addr,
        %handshake_addr,
        "Starting RPC test client"
    );

    if config.workloads.is_empty() {
        warn!("No workloads configured");
        return Ok(());
    }

    let handshake = crate::psk::client(trace_dir)?;

    let subscriber = crate::psk::subscriber(trace_dir);
    let stats = subscriber.0.clone();

    let client: Client =
        s2n_quic_dc::stream::client::tokio::Client::<psk::client::Provider, Subscriber>::builder()
            .with_default_protocol(socket::Protocol::Udp)
            .with_send_buffer(200 * 1024 * 1024)
            .with_recv_buffer(200 * 1024 * 1024)
            .with_send_socket_workers(crate::busy_poll::send_pool().into())
            .with_recv_socket_workers(crate::busy_poll::recv_pool().into())
            .build(handshake, subscriber)?;

    let server_name = crate::psk::server_name();

    let mut handles = Vec::new();

    for workload in config.workloads {
        info!(
            workload = %workload.name,
            workers = workload.workers,
            "Starting workers"
        );

        for worker_id in 0..workload.workers {
            let client = client.clone();
            let workload = workload.clone();
            let stats = stats.clone();
            let server_name = server_name.clone();
            let handle = tokio::spawn(async move {
                run_worker(
                    client,
                    acceptor_addr,
                    handshake_addr,
                    server_name,
                    workload,
                    worker_id,
                    stats,
                )
                .await
            });
            handles.push(handle);
        }
    }

    // Wait for all workers (they run forever)
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

async fn run_worker(
    client: Client,
    acceptor_addr: SocketAddr,
    handshake_addr: SocketAddr,
    server_name: s2n_quic::server::Name,
    workload: WorkloadConfig,
    worker_id: usize,
    stats: crate::stats::Subscriber,
) {
    let delay = if workload.request_delay_ms > 0 {
        Some(Duration::from_millis(workload.request_delay_ms))
    } else {
        None
    };

    loop {
        stats.start_request();
        let (bytes_sent, bytes_received, is_error) = match execute_request(
            &client,
            acceptor_addr,
            handshake_addr,
            server_name.clone(),
            &workload,
        )
        .await
        {
            Ok((sent, received)) => (sent, received, false),
            Err(e) => {
                tracing::error!(
                    workload = %workload.name,
                    worker_id,
                    error = %e,
                    "Request failed"
                );
                (0, 0, true)
            }
        };
        stats.finish_request(bytes_sent, bytes_received, is_error);

        // Delay before next request if configured
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
    }
}

async fn execute_request(
    client: &Client,
    acceptor_addr: SocketAddr,
    handshake_addr: SocketAddr,
    server_name: s2n_quic::server::Name,
    workload: &WorkloadConfig,
) -> io::Result<(u64, u64)> {
    // Connect to the server
    let mut stream = client
        .connect(handshake_addr, acceptor_addr, server_name)
        .await?;

    // Write the 8-byte response size header
    stream.write_u64(workload.response_size).await?;

    // Write request body using Data
    let mut request = Data::new(workload.request_size);
    while !request.is_finished() {
        stream.write_from_fin(&mut request).await?;
    }

    let bytes_sent = 8 + workload.request_size;

    // Read and validate response using Data
    let mut response = Data::new(workload.response_size);
    loop {
        let n = stream.read_into(&mut response).await?;
        if n == 0 {
            break;
        }
    }

    if !response.is_finished() {
        return Err(io::Error::other(format!(
            "response was not fully received: expected {} bytes, got {} bytes",
            workload.response_size,
            response.current_offset()
        )));
    }

    Ok((bytes_sent, workload.response_size))
}
