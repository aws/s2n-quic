// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides TLS support for an endpoint

use cfg_if::cfg_if;
use s2n_quic_core::crypto;

pub trait Provider {
    type Server: 'static + crypto::tls::Endpoint;
    type Client: 'static + crypto::tls::Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    /// Creates a server endpoint for the given provider
    fn start_server(self) -> Result<Self::Server, Self::Error>;

    /// Creates a client endpoint for the given provider
    fn start_client(self) -> Result<Self::Client, Self::Error>;
}

impl_provider_utils!();

cfg_if! {
    if #[cfg(feature = "provider-tls-default")] {
        #[cfg_attr(docsrs, doc(cfg(feature = "provider-tls-default")))]
        pub mod default {
            //! Provides the recommended implementation of TLS using platform detection
            pub use super::default_tls::*;
        }
    } else if #[cfg(feature = "provider-tls-s2n")] {
        pub use s2n_tls as default;
    } else if #[cfg(feature = "provider-tls-rustls")] {
        pub use rustls as default;
    } else {
        pub mod default {
            //! Provides the recommended implementation of TLS using platform detection
            pub use super::default_tls::*;
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
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        // TODO support private key
        let client = default::Client::builder()
            .with_certificate(self.0)?
            .build()?;

        Ok(client)
    }
}

impl Provider for &std::path::Path {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let empty_cert: &[u8] = &[];
        let server = default::Server::builder()
            .with_certificate(empty_cert, self)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let client = default::Client::builder().with_certificate(self)?.build()?;

        Ok(client)
    }
}

impl Provider for (&[u8], &[u8]) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        // TODO support private key
        let client = default::Client::builder()
            .with_certificate(self.0)?
            .build()?;

        Ok(client)
    }
}

impl Provider for &[u8] {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let empty_cert = &[][..];
        let server = default::Server::builder()
            .with_certificate(empty_cert, self)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let client = default::Client::builder().with_certificate(self)?.build()?;

        Ok(client)
    }
}

impl Provider for (&str, &str) {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let server = default::Server::builder()
            .with_certificate(self.0, self.1)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        // TODO support private key
        let client = default::Client::builder()
            .with_certificate(self.0)?
            .build()?;

        Ok(client)
    }
}

impl Provider for &str {
    type Server = <Default as Provider>::Server;
    type Client = <Default as Provider>::Client;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let empty_cert = "";
        let server = default::Server::builder()
            .with_certificate(empty_cert, self)?
            .build()?;

        Ok(server)
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let client = default::Client::builder().with_certificate(self)?.build()?;

        Ok(client)
    }
}

#[cfg(feature = "provider-tls-default")]
mod default_tls {
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
#[cfg(not(feature = "provider-tls-default"))]
mod default_tls {
    // TODO stub out default that fails with error when started
}

#[cfg_attr(docsrs, doc(cfg(feature = "provider-tls-rustls")))]
#[cfg(feature = "s2n-quic-rustls")]
pub mod rustls {
    //! Provides the [rustls](https://docs.rs/rustls/) implementation of TLS

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

#[cfg_attr(docsrs, doc(cfg(feature = "provider-tls-s2n")))]
#[cfg(feature = "s2n-quic-tls")]
pub mod s2n_tls {
    //! Provides the [s2n-tls](https://github.com/aws/s2n-tls) implementation of TLS
    pub use s2n_quic_tls::*;

    impl<L: ConfigLoader> super::Provider for Server<L> {
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

    impl<L: ConfigLoader> super::Provider for Client<L> {
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

#[cfg(feature = "unstable-offload-tls")]
pub mod offload {
    use super::Provider;
    use s2n_quic_core::crypto::tls::offload::OffloadEndpoint;
    pub use s2n_quic_core::crypto::tls::{
        offload::{Executor, ExporterHandler},
        TlsSession,
    };
    use std::sync::Arc;

    pub struct Offload<E, X, H> {
        endpoint: E,
        executor: X,
        exporter: Arc<H>,
    }
    pub struct OffloadBuilder<E, X, H> {
        endpoint: Option<E>,
        executor: Option<X>,
        exporter: Option<Arc<H>>,
    }
    impl<E, X, H> Default for OffloadBuilder<E, X, H> {
        fn default() -> Self {
            Self::new()
        }
    }
    impl<E, X, H> OffloadBuilder<E, X, H> {
        pub fn new() -> Self {
            OffloadBuilder {
                endpoint: None,
                executor: None,
                exporter: None,
            }
        }
        pub fn with_endpoint(mut self, endpoint: E) -> OffloadBuilder<E, X, H> {
            self.endpoint = Some(endpoint);
            self
        }
        pub fn with_executor(mut self, executor: X) -> OffloadBuilder<E, X, H> {
            self.executor = Some(executor);
            self
        }
        pub fn with_exporter(mut self, exporter: H) -> OffloadBuilder<E, X, H> {
            self.exporter = Some(Arc::new(exporter));
            self
        }
        pub fn build(self) -> Offload<E, X, H> {
            Offload {
                endpoint: self.endpoint.expect("Please provide an endpoint"),
                executor: self.executor.expect("Please provide an executor"),
                exporter: self.exporter.expect("Please provide an exporter"),
            }
        }
    }

    impl<E: Provider, X: Executor + Send + 'static, H: ExporterHandler + Send + 'static + Sync>
        Provider for Offload<E, X, H>
    {
        type Server = OffloadEndpoint<<E as Provider>::Server, X, H>;
        type Client = OffloadEndpoint<<E as Provider>::Client, X, H>;
        type Error = E::Error;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            Ok(OffloadEndpoint::new(
                E::start_server(self.endpoint)?,
                self.executor,
                self.exporter,
            ))
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(OffloadEndpoint::new(
                E::start_client(self.endpoint)?,
                self.executor,
                self.exporter,
            ))
        }
    }
}
