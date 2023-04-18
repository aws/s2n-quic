// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rustls::{
    cipher_suite, ClientConfig, Error, RootCertStore, ServerConfig, SupportedCipherSuite,
};
use s2n_quic::provider::{tls, tls::rustls::rustls};
use std::{io::Cursor, path::Path};
use tokio::{fs::File, io::AsyncReadExt};
use tracing::Level;

static PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

pub static DEFAULT_CIPHERSUITES: &[SupportedCipherSuite] = &[
    cipher_suite::TLS13_AES_128_GCM_SHA256,
    cipher_suite::TLS13_AES_256_GCM_SHA384,
    cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
];

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

pub struct MtlsProvider {
    root_store: rustls::RootCertStore,
    my_cert_chain: Vec<rustls::Certificate>,
    my_private_key: rustls::PrivateKey,
}

impl tls::Provider for MtlsProvider {
    type Server = tls::rustls::Server;
    type Client = tls::rustls::Client;
    type Error = rustls::Error;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let verifier = rustls::server::AllowAnyAuthenticatedClient::new(self.root_store);
        let mut cfg = ServerConfig::builder()
            .with_cipher_suites(DEFAULT_CIPHERSUITES)
            .with_safe_default_kx_groups()
            .with_protocol_versions(PROTOCOL_VERSIONS)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(self.my_cert_chain, self.my_private_key)?;

        cfg.ignore_client_order = true;
        cfg.max_fragment_size = None;
        cfg.alpn_protocols = vec![b"h3".to_vec()];
        Ok(cfg.into())
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let mut cfg = ClientConfig::builder()
            .with_cipher_suites(DEFAULT_CIPHERSUITES)
            .with_safe_default_kx_groups()
            .with_protocol_versions(PROTOCOL_VERSIONS)?
            .with_root_certificates(self.root_store)
            .with_single_cert(self.my_cert_chain, self.my_private_key)?;

        cfg.max_fragment_size = None;
        cfg.alpn_protocols = vec![b"h3".to_vec()];
        Ok(cfg.into())
    }
}

impl MtlsProvider {
    pub async fn new<A: AsRef<Path>, B: AsRef<Path>, C: AsRef<Path>>(
        ca_cert_pem: A,
        my_cert_pem: B,
        my_key_pem: C,
    ) -> Result<Self, Error> {
        let root_store = into_root_store(ca_cert_pem.as_ref()).await?;
        let cert_chain = into_certificate(my_cert_pem.as_ref()).await?;
        let private_key = into_private_key(my_key_pem.as_ref()).await?;
        Ok(MtlsProvider {
            root_store,
            my_cert_chain: cert_chain.into_iter().map(rustls::Certificate).collect(),
            my_private_key: rustls::PrivateKey(private_key),
        })
    }
}

async fn into_certificate(path: &Path) -> Result<Vec<Vec<u8>>, Error> {
    let mut f = File::open(path)
        .await
        .map_err(|e| Error::General(format!("Failed to load file: {}", e)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .await
        .map_err(|e| Error::General(format!("Failed to read file: {}", e)))?;
    let mut cursor = Cursor::new(buf);
    let certs = rustls_pemfile::certs(&mut cursor)
        .map(|certs| certs.into_iter().collect())
        .map_err(|_| Error::General("Could not read certificate".to_string()))?;
    Ok(certs)
}

async fn into_root_store(path: &Path) -> Result<RootCertStore, Error> {
    let ca_certs = into_certificate(path).await?;
    let mut cert_store = RootCertStore::empty();
    cert_store.add_parsable_certificates(ca_certs.as_slice());
    Ok(cert_store)
}

async fn into_private_key(path: &Path) -> Result<Vec<u8>, Error> {
    let mut f = File::open(path)
        .await
        .map_err(|e| Error::General(format!("Failed to load file: {}", e)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .await
        .map_err(|e| Error::General(format!("Failed to read file: {}", e)))?;
    let mut cursor = Cursor::new(buf);

    let parsers = [
        rustls_pemfile::rsa_private_keys,
        rustls_pemfile::pkcs8_private_keys,
    ];
    for parser in parsers.iter() {
        cursor.set_position(0);

        match parser(&mut cursor) {
            Ok(keys) if keys.is_empty() => continue,
            Ok(mut keys) if keys.len() == 1 => return Ok(rustls::PrivateKey(keys.pop().unwrap()).0),
            Ok(keys) => {
                return Err(Error::General(format!(
                    "Unexpected number of keys: {} (only 1 supported)",
                    keys.len()
                )));
            }
            // try the next parser
            Err(_) => continue,
        }
    }
    Err(Error::General(
        "could not load any valid private keys".to_string(),
    ))
}
