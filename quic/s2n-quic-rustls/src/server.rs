// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{certificate, cipher_suite::default_crypto_provider, session::Session};
use rustls::{crypto::aws_lc_rs, ConfigBuilder, ServerConfig, WantsVerifier};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::ServerName, crypto::tls};
use std::sync::Arc;

/// Create a QUIC server specific [rustls::ConfigBuilder].
///
/// Uses aws_lc_rs as the crypto provider and sets QUIC specific protocol versions.
pub fn default_config_builder() -> Result<ConfigBuilder<ServerConfig, WantsVerifier>, rustls::Error>
{
    let tls13_cipher_suite_crypto_provider = default_crypto_provider()?;
    ServerConfig::builder_with_provider(tls13_cipher_suite_crypto_provider.into())
        .with_protocol_versions(crate::PROTOCOL_VERSIONS)
}

#[derive(Clone)]
pub struct Server {
    config: Arc<ServerConfig>,
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn builder() -> Builder {
        Builder::new()
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("could not create default server")
    }
}

impl From<ServerConfig> for Server {
    fn from(config: ServerConfig) -> Self {
        Self::new(config)
    }
}

impl From<Arc<ServerConfig>> for Server {
    fn from(config: Arc<ServerConfig>) -> Self {
        Self { config }
    }
}

impl tls::Endpoint for Server {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-8.2
        //# Endpoints MUST send the quic_transport_parameters extension;
        let transport_parameters = transport_parameters.encode_to_vec();

        let session = rustls::quic::ServerConnection::new(
            self.config.clone(),
            crate::QUIC_VERSION,
            transport_parameters,
        )
        .expect("could not create rustls server session");

        Session::new(session.into(), None)
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
        _sni: ServerName,
    ) -> Self::Session {
        panic!("cannot create a client session from a server config");
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_crypto::MAX_TAG_LEN
    }
}

pub struct Builder {
    cert_resolver: Option<Arc<dyn rustls::server::ResolvesServerCert>>,
    application_protocols: Vec<Vec<u8>>,
    key_log: Option<Arc<dyn rustls::KeyLog>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            cert_resolver: None,
            application_protocols: vec![b"h3".to_vec()],
            key_log: None,
        }
    }

    pub fn with_certificate<C: certificate::IntoCertificate, PK: certificate::IntoPrivateKey>(
        self,
        certificate: C,
        private_key: PK,
    ) -> Result<Self, rustls::Error> {
        let certificate = certificate.into_certificate()?;
        let private_key = private_key.into_private_key()?;
        let resolver = AlwaysResolvesChain::new(certificate, private_key)?;
        let resolver = Arc::new(resolver);
        self.with_cert_resolver(resolver)
    }

    pub fn with_cert_resolver(
        mut self,
        cert_resolver: Arc<dyn rustls::server::ResolvesServerCert>,
    ) -> Result<Self, rustls::Error> {
        self.cert_resolver = Some(cert_resolver);
        Ok(self)
    }

    pub fn with_application_protocols<P: Iterator<Item = I>, I: AsRef<[u8]>>(
        mut self,
        protocols: P,
    ) -> Result<Self, rustls::Error> {
        self.application_protocols = protocols.map(|p| p.as_ref().to_vec()).collect();
        Ok(self)
    }

    pub fn with_key_logging(mut self) -> Result<Self, rustls::Error> {
        self.key_log = Some(Arc::new(rustls::KeyLogFile::new()));
        Ok(self)
    }

    pub fn build(self) -> Result<Server, rustls::Error> {
        let builder = default_config_builder()?.with_no_client_auth();

        let mut config = if let Some(cert_resolver) = self.cert_resolver {
            builder.with_cert_resolver(cert_resolver)
        } else {
            return Err(rustls::Error::General(
                "Missing certificate or certificate resolver".to_string(),
            ));
        };

        config.ignore_client_order = true;
        config.max_fragment_size = None;
        config.alpn_protocols = self.application_protocols;

        if let Some(key_log) = self.key_log {
            config.key_log = key_log;
        }

        Ok(Server::new(config))
    }
}

#[derive(Debug)]
struct AlwaysResolvesChain(Arc<rustls::sign::CertifiedKey>);

impl AlwaysResolvesChain {
    fn new(
        chain: certificate::Certificate,
        priv_key: certificate::PrivateKey,
    ) -> Result<Self, rustls::Error> {
        let key = aws_lc_rs::sign::any_supported_type(&priv_key.0)
            .map_err(|_| rustls::Error::General("invalid private key".into()))?;
        Ok(Self(Arc::new(rustls::sign::CertifiedKey::new(
            chain.0, key,
        ))))
    }
}

impl rustls::server::ResolvesServerCert for AlwaysResolvesChain {
    fn resolve(
        &self,
        _client_hello: rustls::server::ClientHello,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}
