// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{
    Server,
    provider::tls::{
        default,
        offload::{Executor, ExporterHandler, OffloadBuilder, TlsSession},
    },
};
use std::error::Error;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
/// NOTE: this certificate is to be used for demonstration purposes only!
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

struct TokioExecutor;
impl Executor for TokioExecutor {
    fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
        tokio::spawn(task);
    }
}
struct Exporter;
impl ExporterHandler for Exporter {
    fn on_tls_handshake_failed(
        &self,
        _session: &impl TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }

    fn on_tls_exporter_ready(
        &self,
        _session: &impl TlsSession,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let tls = default::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)?
        .build()?;

    let tls_endpoint = OffloadBuilder::new()
        .with_endpoint(tls)
        .with_executor(TokioExecutor)
        .with_exporter(Exporter)
        .build();

    let mut server = Server::builder()
        .with_tls(tls_endpoint)?
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
