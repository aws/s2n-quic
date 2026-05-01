// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::ServerConfig;
use s2n_quic_core::stream::testing::Data;
use s2n_quic_dc::psk;
use std::{io, path::PathBuf};
use tokio::io::AsyncReadExt;
use tracing::{error, info};

type Subscriber = crate::psk::Subscriber;
type Server = s2n_quic_dc::stream::server::tokio::Server<psk::server::Provider, Subscriber>;

pub async fn run(config: ServerConfig, trace_dir: &PathBuf) -> io::Result<()> {
    info!("Starting RPC test server");

    // Handshake (QUIC) address uses port - 1 to avoid conflict with the acceptor port
    let mut handshake_bind = config.address;
    handshake_bind.set_port(config.address.port() - 1);
    let handshake = crate::psk::server(handshake_bind, trace_dir).await?;

    let subscriber = crate::psk::subscriber(trace_dir);
    let stats = subscriber.0.clone();

    let server: Server =
        s2n_quic_dc::stream::server::tokio::Server::<psk::server::Provider, Subscriber>::builder()
            .with_address(config.address)
            .with_send_buffer(200 * 1024 * 1024)
            .with_recv_buffer(200 * 1024 * 1024)
            .with_send_socket_workers(crate::busy_poll::send_pool().into())
            .with_recv_socket_workers(crate::busy_poll::recv_pool().into())
            .build(handshake, subscriber)?;

    let acceptor_addr = server.acceptor_addr()?;
    let handshake_addr = server.handshake_addr()?;

    info!(
        %acceptor_addr,
        %handshake_addr,
        "Server listening"
    );

    loop {
        let (stream, peer_addr) = server.accept().await?;

        let stats = stats.clone();
        tokio::spawn(async move {
            stats.start_request();
            let (bytes_received, bytes_sent, is_error) = match handle_connection(stream).await {
                Ok((recv, sent)) => (recv, sent, false),
                Err(e) => {
                    error!(%peer_addr, error = %e, "Error handling connection");
                    (0, 0, true)
                }
            };
            stats.finish_request(bytes_sent, bytes_received, is_error);
        });
    }
}

async fn handle_connection(
    mut stream: s2n_quic_dc::stream::application::Stream<Subscriber>,
) -> io::Result<(u64, u64)> {
    // Read the 8-byte response size header
    let response_size = stream.read_u64().await?;
    let mut total_received = 8u64;

    // Read and validate the rest of the request body using Data
    let mut receiver = Data::new(u64::MAX);
    loop {
        let n = stream.read_into(&mut receiver).await?;
        if n == 0 {
            break;
        }
        total_received += n as u64;
    }

    // Generate and send response data using Data
    let mut response = Data::new(response_size);
    while !response.is_finished() {
        stream.write_from_fin(&mut response).await?;
    }

    Ok((total_received, response_size))
}
