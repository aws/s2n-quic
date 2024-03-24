// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::tls::{
    self as s2n_quic_tls_provider,
    rustls::rustls::{
        // types from the external rustls crate
        pki_types::{CertificateDer, PrivateKeyDer},
        server::WebPkiClientVerifier,
        Error as RustlsError,
        RootCertStore,
    },
};
use std::{io::Cursor, path::Path, sync::Arc};
use tokio::{fs::File, io::AsyncReadExt};
use tracing::Level;

pub fn initialize_logger(endpoint: &str) {
    use std::sync::Once;

    static TRACING: Once = Once::new();

    // make sure this only gets initialized once (per process)
    TRACING.call_once(|| {
        // always write to the same file, and don't rotate it. This would be a
        // bad idea for a long running process, but is useful to make sure that
        // all the logs of our program end up in the same file.
        let file_appender = tracing_appender::rolling::never("logs", format!("{endpoint}.txt"));

        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            // don't color the output, otherwise the text logs will have odd
            // characters
            .with_ansi(false)
            .with_writer(file_appender)
            .init();
    });
}
async fn read_file(path: &Path) -> Result<Vec<u8>, RustlsError> {
    let mut f = File::open(path)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to load file: {}", e)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to read file: {}", e)))?;
    Ok(buf)
}

pub async fn into_certificate(path: &Path) -> Result<Vec<CertificateDer<'static>>, RustlsError> {
    let buf = &read_file(path).await?;
    let mut cursor = Cursor::new(buf);
    rustls_pemfile::certs(&mut cursor)
        .map(|cert| {
            cert.map_err(|_| RustlsError::General("Could not read certificate".to_string()))
        })
        .collect()
}

pub async fn into_root_store(path: &Path) -> Result<RootCertStore, RustlsError> {
    let ca_certs: Vec<CertificateDer<'static>> = into_certificate(path)
        .await
        .map(|certs| certs.into_iter().map(CertificateDer::from))?
        .collect();
    let mut cert_store = RootCertStore::empty();
    cert_store.add_parsable_certificates(ca_certs);
    Ok(cert_store)
}
