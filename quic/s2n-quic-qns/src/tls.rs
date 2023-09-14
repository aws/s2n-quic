// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use std::{path::PathBuf, str::FromStr};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Server {
    #[structopt(long)]
    pub certificate: Option<PathBuf>,

    #[structopt(long)]
    pub private_key: Option<PathBuf>,

    #[structopt(long, default_value)]
    pub tls: TlsProviders,

    /// The key to use for session tickets/PSKs
    ///
    /// Must be at least 16 bytes
    #[structopt(long)]
    pub ticket_key: Option<String>,
}

impl Server {
    #[cfg(unix)]
    pub fn build_s2n_tls(&self, alpns: &[String]) -> Result<s2n_tls::Server<s2n_tls::Server>> {
        // The server builder defaults to a chain because this allows certs to just work, whether
        // the PEM contains a single cert or a chain
        let mut tls = s2n_tls::Server::builder()
            .with_certificate(
                s2n_tls::ca(self.certificate.as_ref())?,
                s2n_tls::private_key(self.private_key.as_ref())?,
            )?
            .with_application_protocols(alpns.iter().map(String::as_bytes))?
            .with_key_logging()?;

        cfg_if::cfg_if! {
            if #[cfg(all(
                s2n_quic_unstable,
                feature = "unstable_client_hello"
            ))] {
                use super::unstable::MyClientHelloHandler;
                let tls = tls.with_client_hello_handler(MyClientHelloHandler {})?;
            }
        }

        if let Some(ticket_key) = &self.ticket_key {
            let config = tls.config_mut();
            config.enable_session_tickets(true)?;
            config.add_session_ticket_key(
                "keyname".as_bytes(),
                ticket_key.as_bytes(),
                std::time::SystemTime::now(),
            )?;
        }

        let server = s2n_tls::Server::from_loader(tls.build()?);
        Ok(server)
    }

    pub fn build_rustls(&self, alpns: &[String]) -> Result<rustls::Server> {
        // The server builder defaults to a chain because this allows certs to just work, whether
        // the PEM contains a single cert or a chain
        let tls = rustls::Server::builder()
            .with_certificate(
                rustls::ca(self.certificate.as_ref())?,
                rustls::private_key(self.private_key.as_ref())?,
            )?
            .with_application_protocols(alpns.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        Ok(tls)
    }

    pub fn build_null(&self) -> Result<null::Provider> {
        Ok(null::Provider)
    }
}

#[derive(Debug, StructOpt)]
pub struct Client {
    #[structopt(long)]
    pub ca: Option<PathBuf>,

    #[structopt(long, default_value)]
    pub tls: TlsProviders,

    /// disable verification of the server certificate (rustls only)
    #[structopt(long)]
    pub disable_cert_verification: bool,
}

impl Client {
    #[cfg(unix)]
    pub fn build_s2n_tls(&self, alpns: &[String]) -> Result<s2n_tls::Client> {
        let tls = s2n_tls::Client::builder()
            .with_certificate(s2n_tls::ca(self.ca.as_ref())?)?
            // the "amplificationlimit" tests generates a very large chain so bump the limit
            .with_max_cert_chain_depth(10)?
            .with_application_protocols(alpns.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        Ok(tls)
    }

    pub fn build_rustls(&self, alpns: &[String]) -> Result<rustls::Client> {
        let tls = if self.disable_cert_verification {
            use ::rustls::{version, ClientConfig, KeyLogFile};
            use std::sync::Arc;

            let mut config = ClientConfig::builder()
                .with_cipher_suites(rustls::DEFAULT_CIPHERSUITES)
                .with_safe_default_kx_groups()
                .with_protocol_versions(&[&version::TLS13])?
                .with_custom_certificate_verifier(Arc::new(rustls::DisabledVerifier))
                .with_no_client_auth();
            config.max_fragment_size = None;
            config.alpn_protocols = alpns.iter().map(|p| p.as_bytes().to_vec()).collect();
            config.key_log = Arc::new(KeyLogFile::new());
            rustls::Client::new(config)
        } else {
            rustls::Client::builder()
                .with_certificate(rustls::ca(self.ca.as_ref())?)?
                .with_application_protocols(alpns.iter().map(String::as_bytes))?
                .with_key_logging()?
                .build()?
        };

        Ok(tls)
    }

    pub fn build_null(&self) -> Result<null::Provider> {
        Ok(null::Provider)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TlsProviders {
    /// Use s2n-tls as the tls provider
    #[cfg(unix)]
    S2N,
    /// Use rustls as the tls provider
    Rustls,
    /// Use the null tls provider
    Null,
}

impl Default for TlsProviders {
    fn default() -> Self {
        #[cfg(unix)]
        return Self::S2N;
        #[cfg(not(unix))]
        return Self::Rustls;
    }
}

impl ToString for TlsProviders {
    fn to_string(&self) -> String {
        match self {
            #[cfg(unix)]
            TlsProviders::S2N => String::from("s2n-tls"),
            TlsProviders::Rustls => String::from("rustls"),
            TlsProviders::Null => String::from("null"),
        }
    }
}

impl FromStr for TlsProviders {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "rustls" => Self::Rustls,
            "null" => Self::Null,
            #[cfg(unix)]
            "s2n-tls" => Self::S2N,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unsupported tls provider: {s}"),
                )
                .into())
            }
        })
    }
}

pub mod default {
    #[cfg(not(unix))]
    pub use super::rustls::*;
    #[cfg(unix)]
    pub use super::s2n_tls::*;
}

#[cfg(unix)]
pub mod s2n_tls {
    use super::*;
    pub use s2n_quic::provider::tls::s2n_tls::{
        certificate::{Certificate, IntoCertificate, IntoPrivateKey, PrivateKey},
        Client, Server,
    };

    pub fn ca(ca: Option<&PathBuf>) -> Result<Certificate> {
        Ok(if let Some(pathbuf) = ca.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    pub fn private_key(private_key: Option<&PathBuf>) -> Result<PrivateKey> {
        Ok(if let Some(pathbuf) = private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }
}

pub mod rustls {
    use super::*;
    pub use s2n_quic::provider::tls::rustls::{
        certificate::{Certificate, IntoCertificate, IntoPrivateKey, PrivateKey},
        Client, Server, DEFAULT_CIPHERSUITES,
    };

    pub fn ca(ca: Option<&PathBuf>) -> Result<Certificate> {
        Ok(if let Some(pathbuf) = ca.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    pub fn private_key(private_key: Option<&PathBuf>) -> Result<PrivateKey> {
        Ok(if let Some(pathbuf) = private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }

    pub struct DisabledVerifier;

    impl ::rustls::client::ServerCertVerifier for DisabledVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &::rustls::Certificate,
            _intermediates: &[::rustls::Certificate],
            _server_name: &::rustls::ServerName,
            _scts: &mut dyn Iterator<Item = &[u8]>,
            _ocsp_response: &[u8],
            _now: std::time::SystemTime,
        ) -> Result<::rustls::client::ServerCertVerified, ::rustls::Error> {
            Ok(::rustls::client::ServerCertVerified::assertion())
        }
    }
}

pub mod null {
    use super::*;
    use s2n_quic_core::crypto::tls::null::*;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl s2n_quic::provider::tls::Provider for Provider {
        type Server = Endpoint;
        type Client = Endpoint;
        type Error = String;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            Ok(Self::Server::default())
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(Self::Client::default())
        }
    }
}
