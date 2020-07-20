use s2n_quic_core::crypto;

/// Provides TLS support for an endpoint
pub trait Provider {
    type Server: crypto::tls::Endpoint;
    type Client: crypto::tls::Endpoint;
    type Error;

    fn server(self) -> Result<Self::Server, Self::Error>;
    fn client(self) -> Result<Self::Client, Self::Error>;
}

#[cfg(feature = "rustls")]
pub use rustls as default;

#[derive(Debug, Default)]
pub struct Default;

impl Provider for Default {
    type Server = default::Server;
    type Client = default::Client;
    type Error = core::convert::Infallible;

    fn server(self) -> Result<Self::Server, Self::Error> {
        Ok(default::Server::default())
    }

    fn client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

impl Provider for (&std::path::Path, &std::path::Path) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error>;

    fn server(self) -> Result<Self::Server, Self::Error> {
        let cert = std::fs::read(self.0)?;
        let key = std::fs::read(self.1)?;

        let server = default::Server::builder()
            .with_certificate(cert, key)?
            .build()?;

        Ok(server)
    }

    fn client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

impl_provider_utils!();

#[cfg(feature = "rustls")]
pub mod rustls {
    pub use s2n_quic_rustls::{rustls::TLSError, *};

    impl super::Provider for Server {
        type Server = Self;
        type Client = Client;
        type Error = TLSError;

        fn server(self) -> Result<Self::Server, Self::Error> {
            Ok(self)
        }

        fn client(self) -> Result<Self::Client, Self::Error> {
            panic!("cannot create a client from a server");
        }
    }

    impl super::Provider for Client {
        type Server = Server;
        type Client = Self;
        type Error = TLSError;

        fn server(self) -> Result<Self::Server, Self::Error> {
            panic!("cannot create a server from a client");
        }

        fn client(self) -> Result<Self::Client, Self::Error> {
            Ok(self)
        }
    }
}
