use cfg_if::cfg_if;
use s2n_quic_core::crypto;

/// Provides TLS support for an endpoint
pub trait Provider {
    type Server: 'static + crypto::tls::Endpoint;
    type Client: 'static + crypto::tls::Endpoint;
    type Error: 'static + core::fmt::Display;

    /// Creates a server endpoint for the given provider
    fn start_server(self) -> Result<Self::Server, Self::Error>;

    /// Creates a client endpoint for the given provider
    fn start_client(self) -> Result<Self::Client, Self::Error>;
}

impl_provider_utils!();

cfg_if! {
    // TODO prefer s2n-tls
    if #[cfg(feature = "rustls")] {
        pub use rustls as default;
    } else {
        pub mod default {
            // TODO stub out implementations that panic on initialization
        }
    }
}

#[derive(Debug, Default)]
pub struct Default;

impl Provider for Default {
    type Server = default::Server;
    type Client = default::Client;
    type Error = core::convert::Infallible;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        Ok(Self::Server::default())
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(Self::Client::default())
    }
}

impl Provider for (&std::path::Path, &std::path::Path) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let cert = std::fs::read(self.0)?;
        let key = std::fs::read(self.1)?;

        let server = default::Server::builder()
            .with_certificate(cert, key)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

impl Provider for (&[u8], &[u8]) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let cert = self.0.to_vec();
        let key = self.1.to_vec();

        let server = default::Server::builder()
            .with_certificate(cert, key)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

#[cfg(feature = "rustls")]
pub mod rustls {
    pub use s2n_quic_rustls::{rustls::TLSError, *};

    impl super::Provider for Server {
        type Server = Self;
        type Client = Client;
        type Error = TLSError;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            Ok(self)
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            panic!("cannot create a client from a server");
        }
    }

    impl super::Provider for Client {
        type Server = Server;
        type Client = Self;
        type Error = TLSError;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            panic!("cannot create a server from a client");
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(self)
        }
    }
}
