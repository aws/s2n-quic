// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use dos_mitigation::example;
use s2n_quic::Server;
use std::{error::Error, time::Duration};

pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Limit the duration any handshake attempt may take to 5 seconds
    // By default, handshakes are limited to 10 seconds.
    let connection_limits = s2n_quic::provider::limits::Limits::new()
        .with_max_handshake_duration(Duration::from_secs(5))
        .expect("connection limits are valid");

    // Limit the number of inflight handshakes to 100.
    let endpoint_limits = s2n_quic::provider::endpoint_limits::Default::builder()
        .with_inflight_handshake_limit(100)?
        .build()?;

    // Build an `s2n_quic::Server`
    let mut server = Server::builder()
        // Provide the `connection_limits` defined above
        .with_limits(connection_limits)?
        // Provide the `endpoint_limits defined above
        .with_endpoint_limits(endpoint_limits)?
        // Provide a tuple of the `example::MyConnectionSupervisor` defined in `dos-mitigation/src/lib.rs`
        // and the default event tracing subscriber. This combination will allow for both the DDoS mitigation
        // functionality of `MyConnectionSupervisor` as well as event tracing to be utilized.
        .with_event((
            example::MyConnectionSupervisor,
            s2n_quic::provider::event::tracing::Subscriber::default(),
        ))?
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io("127.0.0.1:4433")?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            eprintln!("Connection accepted from {:?}", connection.remote_addr());

            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
                    eprintln!("Stream opened from {:?}", stream.connection().remote_addr());

                    // echo any data back to the stream
                    while let Ok(Some(data)) = stream.receive().await {
                        stream.send(data).await.expect("stream should be open");
                    }
                });
            }
        });
    }

    Ok(())
}
