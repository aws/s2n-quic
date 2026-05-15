// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ClientConfig, WorkloadConfig};
use s2n_quic_core::{
    buffer::{reader::storage::Storage as _, Reader as _},
    stream::testing::Data,
    varint::VarInt,
};
use s2n_quic_dc::stream3::endpoint::Endpoint;
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tracing::{info, warn};

pub async fn run(
    endpoint: Arc<Endpoint>,
    config: ClientConfig,
    server_addr: SocketAddr,
) -> io::Result<()> {
    info!(
        workload_count = config.workloads.len(),
        %server_addr,
        "Starting stream3 RPC test client"
    );

    if config.workloads.is_empty() {
        warn!("No workloads configured");
        return Ok(());
    }

    let data_addrs = endpoint.data_addrs.clone();

    // Create PSK client provider with data addrs
    let handshake = crate::psk::client(data_addrs, endpoint.path_secret_map.clone())?;

    // Create stream3 client
    let server_name = crate::psk::server_name();
    let client = s2n_quic_dc::stream3::Client::new(endpoint, handshake, server_name);

    let stats = crate::stats::Subscriber::spawn(std::time::Duration::from_secs(1));

    let mut handles = Vec::new();

    for workload in config.workloads {
        info!(
            workload = %workload.name,
            workers = workload.workers,
            "Starting workers"
        );

        for worker_id in 0..workload.workers {
            let mut client = client.clone();
            let workload = workload.clone();
            let stats = stats.clone();
            let handle = tokio::spawn(async move {
                run_worker(&mut client, server_addr, workload, worker_id, stats).await
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
    client: &mut s2n_quic_dc::stream3::Client,
    server_addr: SocketAddr,
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
        let (bytes_sent, bytes_received, is_error) =
            match execute_request(client, server_addr, &workload).await {
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

        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
    }
}

async fn execute_request(
    client: &mut s2n_quic_dc::stream3::Client,
    server_addr: SocketAddr,
    workload: &WorkloadConfig,
) -> io::Result<(u64, u64)> {
    // Connect to the server — handshake address is used to obtain/cache path secrets,
    // data address is derived from the path secret entry
    let stream = client.connect(server_addr, VarInt::ZERO).await?;
    let (mut reader, mut writer) = stream.into_split();

    let request_size = workload.request_size;
    let response_size = workload.response_size;

    // Send the request concurrently with receiving the response so both halves
    // are exercised at the same time, covering more half-close code paths.
    let send = async move {
        let header = response_size.to_be_bytes();
        let mut payload = (&header[..]).chain(Data::new(request_size));
        writer.write_all_from_fin(&mut payload).await?;

        io::Result::Ok(8 + request_size)
    };

    let recv = async move {
        // Read and validate response using Data
        let mut response = Data::new(response_size);
        loop {
            let n = reader.read_into(&mut response).await?;
            if n == 0 {
                break;
            }
        }

        if !response.is_finished() {
            return Err(io::Error::other(format!(
                "response was not fully received: expected {} bytes, got {} bytes",
                response_size,
                response.current_offset()
            )));
        }

        io::Result::Ok(response_size)
    };

    let (bytes_sent, bytes_received) = tokio::try_join!(send, recv)?;
    Ok((bytes_sent, bytes_received))
}
