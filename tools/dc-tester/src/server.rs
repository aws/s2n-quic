// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{stream::testing::Data, varint::VarInt};
use s2n_quic_dc::stream2::endpoint::Endpoint;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::io::AsyncReadExt as _;
use tracing::{error, info};

pub async fn run(endpoint: Arc<Endpoint>, address: SocketAddr) -> io::Result<()> {
    info!("Starting stream2 RPC test server");

    let data_port = endpoint.data_port;

    // Create PSK server provider — address is the well-known server address,
    // data_port is advertised to peers so they know where to send data
    let handshake =
        crate::psk::server(address, data_port, endpoint.path_secret_map.clone()).await?;

    // Create stream2 server
    let server = s2n_quic_dc::stream2::Server::new(endpoint, handshake);

    // Register channel acceptor with ID 0
    let accept_rx = server.register_acceptor_channel(VarInt::ZERO, 1024)?;

    info!(
        %address,
        data_port,
        "Server listening"
    );

    let stats = crate::stats::Subscriber::spawn(std::time::Duration::from_secs(1));

    loop {
        let stream = accept_rx.recv_front().await.map_err(|_| {
            io::Error::new(io::ErrorKind::ConnectionAborted, "acceptor channel closed")
        })?;

        let stats = stats.clone();
        tokio::spawn(async move {
            stats.start_request();
            let (bytes_received, bytes_sent, is_error) = match handle_connection(stream).await {
                Ok((recv, sent)) => (recv, sent, false),
                Err(e) => {
                    error!(error = %e, "Error handling connection");
                    (0, 0, true)
                }
            };
            stats.finish_request(bytes_sent, bytes_received, is_error);
        });
    }
}

async fn handle_connection(mut stream: s2n_quic_dc::stream2::Stream) -> io::Result<(u64, u64)> {
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
