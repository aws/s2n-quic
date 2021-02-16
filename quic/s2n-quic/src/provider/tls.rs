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
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
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
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

impl Provider for (&str, &str) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(default::Client::default())
    }
}

#[cfg(feature = "default-tls-provider")]
pub mod default {
    pub use s2n_quic_tls_default::*;

    // We need to implement the provider trait for whatever the default is as long as it's
    // not called out explicitly and we're not on the platform that uses it by default.
    //
    // Note: I know this looks like a mess. And it is. Hopefully in the future cargo will support
    // platform-specific default features.
    #[cfg(not(any(
        all(not(unix), feature = "s2n-quic-rustls"),
        all(unix, feature = "s2n-quic-tls")
    )))]
    mod default_provider {
        use super::*;
        use crate::provider::tls;

        impl tls::Provider for Server {
            type Server = Self;
            type Client = Client;
            type Error = core::convert::Infallible;

            fn start_server(self) -> Result<Self::Server, Self::Error> {
                Ok(self)
            }

            fn start_client(self) -> Result<Self::Client, Self::Error> {
                panic!("cannot create a client from a server");
            }
        }

        impl tls::Provider for Client {
            type Server = Server;
            type Client = Self;
            type Error = core::convert::Infallible;

            fn start_server(self) -> Result<Self::Server, Self::Error> {
                panic!("cannot create a server from a client");
            }

            fn start_client(self) -> Result<Self::Client, Self::Error> {
                Ok(self)
            }
        }
    }
}
#[cfg(not(feature = "default-tls-provider"))]
pub mod default {
    // TODO stub out default that fails with error when started
}

#[cfg(feature = "rustls")]
pub mod rustls {
    pub use s2n_quic_rustls::*;

    impl super::Provider for Server {
        type Server = Self;
        type Client = Client;
        type Error = core::convert::Infallible;

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
        type Error = core::convert::Infallible;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            panic!("cannot create a server from a client");
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(self)
        }
    }
}

#[cfg(feature = "s2n-quic-tls")]
pub mod s2n_tls {
    pub use s2n_quic_tls::*;

    impl super::Provider for Server {
        type Server = Self;
        type Client = Client;
        type Error = core::convert::Infallible;

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
        type Error = core::convert::Infallible;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            panic!("cannot create a server from a client");
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(self)
        }
    }
}
