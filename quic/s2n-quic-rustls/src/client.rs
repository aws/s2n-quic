// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{certificate, cipher_suite::default_crypto_provider, session::Session};
use core::convert::TryFrom;
use rustls::{ClientConfig, ConfigBuilder, WantsVerifier};
use s2n_codec::EncoderValue;
use s2n_quic_core::{application::ServerName, crypto::tls};
use std::sync::Arc;

/// Create a QUIC client specific [rustls::ConfigBuilder].
///
/// Uses aws_lc_rs as the crypto provider and sets QUIC specific protocol versions.
pub fn default_config_builder() -> Result<ConfigBuilder<ClientConfig, WantsVerifier>, rustls::Error>
{
    let tls13_cipher_suite_crypto_provider = default_crypto_provider()?;
    ClientConfig::builder_with_provider(tls13_cipher_suite_crypto_provider.into())
        .with_protocol_versions(crate::PROTOCOL_VERSIONS)
}

#[derive(Clone)]
pub struct Client {
    config: Arc<ClientConfig>,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub fn builder() -> Builder {
        Builder::new()
    }
}

impl Default for Client {
    fn default() -> Self {
        // TODO this will currently panic since there is no default root cert
        Self::builder()
            .build()
            .expect("could not create default client")
    }
}

impl From<ClientConfig> for Client {
    fn from(config: ClientConfig) -> Self {
        Self::new(config)
    }
}

impl From<Arc<ClientConfig>> for Client {
    fn from(config: Arc<ClientConfig>) -> Self {
        Self { config }
    }
}

impl tls::Endpoint for Client {
    type Session = Session;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
    ) -> Self::Session {
        panic!("cannot create a server session from a client config");
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: ServerName,
    ) -> Self::Session {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-8.2
        //# Endpoints MUST send the quic_transport_parameters extension;
        let transport_parameters = transport_parameters.encode_to_vec();

        let rustls_server_name = rustls::pki_types::ServerName::try_from(server_name.to_string())
            .expect("invalid server name");

        let session = rustls::quic::ClientConnection::new(
            self.config.clone(),
            crate::QUIC_VERSION,
            rustls_server_name,
            transport_parameters,
        )
        .expect("could not create rustls client session");

        Session::new(session.into(), Some(server_name))
    }

    fn max_tag_length(&self) -> usize {
        s2n_quic_crypto::MAX_TAG_LEN
    }
}

pub struct Builder {
    cert_store: rustls::RootCertStore,
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
            cert_store: rustls::RootCertStore::empty(),
            application_protocols: vec![b"h3".to_vec()],
            key_log: None,
        }
    }

    pub fn with_certificate<C: certificate::IntoCertificate>(
        mut self,
        certificate: C,
    ) -> Result<Self, rustls::Error> {
        let certificates = certificate.into_certificate()?;
        let root_certificate = certificates.0.first().ok_or_else(|| {
            rustls::Error::General("Certificate chain needs to have at least one entry".to_string())
        })?;
        self.cert_store
            .add(root_certificate.to_owned())
            .map_err(|err| rustls::Error::General(err.to_string()))?;
        Ok(self)
    }

    pub fn with_max_cert_chain_depth(self, len: u16) -> Result<Self, rustls::Error> {
        // TODO is there a way to configure this?
        let _ = len;
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

    pub fn build(self) -> Result<Client, rustls::Error> {
        // TODO load system root store?
        if self.cert_store.is_empty() {
            //= https://www.rfc-editor.org/rfc/rfc9001#section-4.4
            //# A client MUST authenticate the identity of the server.
            return Err(rustls::Error::General(
                "missing trusted root certificate(s)".to_string(),
            ));
        }

        let mut config = default_config_builder()?
            .with_root_certificates(self.cert_store)
            .with_no_client_auth();

        config.max_fragment_size = None;
        config.alpn_protocols = self.application_protocols;

        if let Some(key_log) = self.key_log {
            config.key_log = key_log;
        }

        Ok(Client::new(config))
    }
}
