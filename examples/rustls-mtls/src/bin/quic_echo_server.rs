// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rustls_mtls::{initialize_logger, MtlsProvider};
use s2n_quic::Server;
use std::error::Error;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CACERT_PEM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/certs/ca-cert.pem");
pub static MY_CERT_PEM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/certs/server-cert.pem");
pub static MY_KEY_PEM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/certs/server-key.pem");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    initialize_logger("server");
    let provider = MtlsProvider::new(CACERT_PEM, MY_CERT_PEM, MY_KEY_PEM).await?;
    let mut server = Server::builder()
        .with_event(s2n_quic::provider::event::tracing::Subscriber::default())?
        .with_tls(provider)?
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
