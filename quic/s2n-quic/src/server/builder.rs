// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    provider::*,
    server::{DefaultProviders, Server, ServerProviders},
};
use core::fmt;

/// A builder for configuring [`Server`] providers
#[derive(Debug)]
pub struct Builder<Providers>(Providers);

impl Default for Builder<DefaultProviders> {
    fn default() -> Self {
        Self(Default::default())
    }
}

/// An error indicating a failure to start a [`Server`]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
#[non_exhaustive]
pub struct StartError(Box<dyn 'static + fmt::Display>);

impl StartError {
    pub(crate) fn new<T: 'static + fmt::Display>(error: T) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Debug for StartError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("StartError")
            .field(&format_args!("{}", self.0))
            .finish()
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<Providers: ServerProviders> Builder<Providers> {
    impl_provider_method!(
        /// Sets the congestion controller provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the congestion controller to `Reno` with the default configuration.
        ///
        /// ```rust,ignore
        /// # use std::error::Error;
        /// use s2n_quic::{Server, provider::congestion_controller};
        ///
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_congestion_controller(congestion_controller::Reno::default())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_congestion_controller,
        congestion_controller
    );

    impl_provider_method!(
        /// Sets the connection ID provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom connection ID provider for the server
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_connection_id(MyConnectionIDFormat::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_connection_id,
        connection_id
    );

    impl_provider_method!(
        /// Sets the stateless reset token provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom stateless reset token provider for the server
        ///
        /// ```rust,ignore
        /// # use std::error::Error;
        /// use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_stateless_reset_token(MyStatelessResetTokenGenerator::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_stateless_reset_token,
        stateless_reset_token
    );

    impl_provider_method!(
        /// Sets the limits provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the max idle time, while inheriting the remaining default limits
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::limits};
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_limits(limits::Default::default())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_limits,
        limits
    );

    impl_provider_method!(
        /// Sets the endpoint limits provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the max inflight handshakes for an endpoint, while inheriting the remaining default limits
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::endpoint_limits};
        ///
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_endpoint_limits(endpoint_limits::Default::default())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_endpoint_limits,
        endpoint_limits
    );

    impl_provider_method!(
        /// Sets the event provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom event subscriber for the server
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_event(MyEventLogger::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_event,
        event
    );

    impl_provider_method!(
        /// Sets the token provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom token provider for the server
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_token(MyTokenProvider::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_token,
        token
    );

    impl_provider_method!(
        /// Sets the IO provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Starts listening on [`127.0.0.1:443`](https://127.0.0.1)
        ///
        /// ```rust,no_run
        /// # use std::error::Error;
        /// use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_io("127.0.0.1:443")?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Configures a socket with the provided `Builder`
        ///
        /// ```rust,no_run
        /// # use std::error::Error;
        /// use s2n_quic::{Server, provider::io::tokio::Builder as IoBuilder};
        /// use std::net::ToSocketAddrs;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let addr = "127.0.0.1:443".to_socket_addrs()?.next().unwrap();
        ///
        /// let io = IoBuilder::default()
        ///     .with_address(addr)?
        ///     .build()?;
        ///
        /// let server = Server::builder()
        ///     .with_io(io)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_io,
        io
    );

    impl_provider_method!(
        /// Sets the TLS provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// The default TLS provider and configuration will be used with the
        /// path to the private key.
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, path::Path};
        /// # use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Sets the TLS provider to a TLS server builder
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, path::Path};
        /// # use s2n_quic::Server;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let tls = s2n::tls::Server::builder()
        ///     .with_certificate(Path::new("./certs/cert.pem"), Path::new("./certs/key.pem"))?
        ///     .with_security_policy(s2n::tls::security_policy::S2N_20190802)?;
        ///
        /// let server = Server::builder()
        ///     .with_tls(tls)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_tls,
        tls
    );

    /// Starts the [`Server`] with the configured providers
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::{error::Error, path::Path};
    /// # use s2n_quic::Server;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::builder()
    ///     .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
    ///     .with_io("127.0.0.1:443")?
    ///     .start()?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn start(self) -> Result<Server, StartError> {
        let acceptor = self.0.build().start()?;
        Ok(Server { acceptor })
    }
}
