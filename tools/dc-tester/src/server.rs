// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{stream::testing::Data, varint::VarInt};
use s2n_quic_dc::stream3::endpoint::Endpoint;
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{io::AsyncReadExt as _, task::JoinSet};
use tracing::{error, info};

pub async fn run(endpoint: Arc<Endpoint>, address: SocketAddr) -> io::Result<()> {
    info!("Starting stream3 RPC test server");

    let data_addrs = endpoint.data_addrs.clone();
    let num_recv_workers = data_addrs.len();

    // Create PSK server provider — address is the well-known server address,
    // data_addrs are advertised to peers so they know where to send data
    let handshake =
        crate::psk::server(address, data_addrs, endpoint.path_secret_map.clone()).await?;

    // Create stream3 server
    let server = s2n_quic_dc::stream3::Server::new(endpoint, handshake);

    // Register channel acceptor with ID 0
    let accept_rx = server.register_acceptor_channel(VarInt::ZERO, (u32::MAX as usize).into())?;

    info!(
        %address,
        recv_workers = num_recv_workers,
        "Server listening"
    );

    let stats = crate::stats::Subscriber::spawn(std::time::Duration::from_secs(1));

    let mut tasks = JoinSet::new();

    for _ in 0..16 {
        let mut accept_rx = accept_rx.clone();
        let stats = stats.clone();
        tasks.spawn(async move {
            loop {
                let Some(stream) = accept_rx.recv().await else {
                    return Err::<(), io::Error>(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "acceptor channel closed",
                    ));
                };

                let stats = stats.clone();
                tokio::spawn(async move {
                    stats.start_request();
                    let (bytes_received, bytes_sent, is_error) =
                        match handle_connection(stream).await {
                            Ok((recv, sent)) => (recv, sent, false),
                            Err(e) => {
                                error!(error = %e, "Error handling connection");
                                (0, 0, true)
                            }
                        };
                    stats.finish_request(bytes_sent, bytes_received, is_error);
                });
            }
        });
    }

    let res = tasks
        .join_next()
        .await
        .unwrap()
        .unwrap_or_else(|e| Err(io::Error::other(e)));

    tasks.abort_all();

    res
}

async fn handle_connection(stream: s2n_quic_dc::stream3::Stream) -> io::Result<(u64, u64)> {
    let (mut reader, mut writer) = stream.into_split();

    tokio::time::timeout(Duration::from_secs(1), reader.validate())
        .await
        .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))?;

    // Read the 8-byte response size header (required by the send half to know how many bytes to write)
    let response_size = reader.read_u64().await?;

    // Read the remaining request body and write the response concurrently so both
    // halves are exercised at the same time, covering more half-close code paths.
    let recv = async move {
        let mut total_received = 8u64;
        let mut receiver = Data::new(u64::MAX);
        loop {
            let n = reader.read_into(&mut receiver).await?;
            if n == 0 {
                break;
            }
            total_received += n as u64;
        }
        // reader drops here; if the request wasn't fully drained, drop sends STOP_SENDING
        io::Result::Ok(total_received)
    };

    let send = async move {
        // write_from_fin transmits FIN on the final chunk; writer drop calls shutdown()
        // as a fallback if FIN hasn't been sent yet (e.g. empty response)
        let mut response = Data::new(response_size);
        while !response.is_finished() {
            writer.write_from_fin(&mut response).await?;
        }
        io::Result::Ok(response_size)
    };

    let (total_received, bytes_sent) = tokio::try_join!(recv, send)?;
    Ok((total_received, bytes_sent))
}
