// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::tls::{
    self,
    default::default_crypto_provider,
    rustls::rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        server::WebPkiClientVerifier,
        Error as RustlsError, RootCertStore,
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

pub struct MtlsProvider {
    root_store: RootCertStore,
    my_cert_chain: Vec<CertificateDer<'static>>,
    my_private_key: PrivateKeyDer<'static>,
}

impl tls::Provider for MtlsProvider {
    type Server = tls::rustls::Server;
    type Client = tls::rustls::Client;
    type Error = RustlsError;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let tls13_cipher_suite_crypto_provider = default_crypto_provider()?;
        let verifier = WebPkiClientVerifier::builder_with_provider(
            Arc::new(self.root_store),
            tls13_cipher_suite_crypto_provider.into(),
        )
        .build()
        .unwrap();
        let mut cfg = tls::rustls::server_config_builder()?
            .with_client_cert_verifier(verifier)
            .with_single_cert(self.my_cert_chain, self.my_private_key)?;

        cfg.ignore_client_order = true;
        cfg.max_fragment_size = None;
        cfg.alpn_protocols = vec![b"h3".to_vec()];
        Ok(cfg.into())
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let mut cfg = tls::rustls::client_config_builder()?
            .with_root_certificates(self.root_store)
            .with_client_auth_cert(self.my_cert_chain, self.my_private_key)?;

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
    ) -> Result<Self, RustlsError> {
        let root_store = into_root_store(ca_cert_pem.as_ref()).await?;
        let cert_chain = into_certificate(my_cert_pem.as_ref()).await?;
        let private_key = into_private_key(my_key_pem.as_ref()).await?;
        Ok(MtlsProvider {
            root_store,
            my_cert_chain: cert_chain.into_iter().map(CertificateDer::from).collect(),
            my_private_key: private_key,
        })
    }
}

async fn into_certificate(path: &Path) -> Result<Vec<Vec<u8>>, RustlsError> {
    let mut f = File::open(path)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to load file: {}", e)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to read file: {}", e)))?;
    let mut cursor = Cursor::new(buf);
    let certs = rustls_pemfile::certs(&mut cursor)
        .map(|certs| certs.into_iter().collect())
        .map_err(|_| RustlsError::General("Could not read certificate".to_string()))?;
    Ok(certs)
}

async fn into_root_store(path: &Path) -> Result<RootCertStore, RustlsError> {
    let ca_certs: Vec<CertificateDer<'static>> = into_certificate(path)
        .await
        .map(|certs| certs.into_iter().map(CertificateDer::from))?
        .collect();
    let mut cert_store = RootCertStore::empty();
    cert_store.add_parsable_certificates(ca_certs);
    Ok(cert_store)
}

fn construct_pkcs1_key(key: Vec<u8>) -> Result<PrivateKeyDer<'static>, RustlsError> {
    Ok(PrivateKeyDer::Pkcs1(key.into()))
}

fn construct_pkcs8_key(key: Vec<u8>) -> Result<PrivateKeyDer<'static>, RustlsError> {
    Ok(PrivateKeyDer::Pkcs8(key.into()))
}

async fn into_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, RustlsError> {
    let mut f = File::open(path)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to load file: {}", e)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .await
        .map_err(|e| RustlsError::General(format!("Failed to read file: {}", e)))?;
    let mut cursor = Cursor::new(buf);

    macro_rules! parse_key {
        ($parser:ident, $constructor:ident) => {
            cursor.set_position(0);

            match rustls_pemfile::$parser(&mut cursor) {
                // try the next parser
                Err(_) => (),
                // try the next parser
                Ok(keys) if keys.is_empty() => (),
                Ok(mut keys) if keys.len() == 1 => {
                    return $constructor(keys.pop().unwrap());
                }
                Ok(keys) => {
                    return Err(RustlsError::General(format!(
                        "Unexpected number of keys: {} (only 1 supported)",
                        keys.len()
                    )));
                }
            }
        };
    }

    // attempt to parse PKCS8 encoded key
    parse_key!(pkcs8_private_keys, construct_pkcs8_key);
    // attempt to parse RSA key
    parse_key!(rsa_private_keys, construct_pkcs1_key);

    Err(RustlsError::General(
        "could not load any valid private keys".to_string(),
    ))
}
