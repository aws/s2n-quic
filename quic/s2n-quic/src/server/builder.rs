// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    provider::*,
    server::{DefaultProviders, Server, ServerProviders},
};

/// A builder for configuring [`Server`] providers
#[derive(Debug)]
pub struct Builder<Providers>(pub(crate) Providers);

impl Default for Builder<DefaultProviders> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Providers: ServerProviders> Builder<Providers> {
    impl_provider_method!(
        /// Sets the connection ID provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Uses the default connection ID provider with the default configuration.
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::connection_id};
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_connection_id(connection_id::Default::default())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Sets a custom connection ID provider for the server
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::connection_id};
        /// use rand::prelude::*;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// struct MyConnectionIdFormat;
        ///
        /// impl connection_id::Generator for MyConnectionIdFormat {
        ///     fn generate(&mut self, _conn_info: &connection_id::ConnectionInfo) -> connection_id::LocalId {
        ///         let mut id = [0u8; 16];
        ///         rand::rng().fill_bytes(&mut id);
        ///         connection_id::LocalId::try_from_bytes(&id[..]).unwrap()
        ///     }
        /// }
        ///
        /// impl connection_id::Validator for MyConnectionIdFormat {
        ///     fn validate(&self, _conn_info: &connection_id::ConnectionInfo, packet: &[u8]) -> Option<usize> {
        ///         // this connection id format is always 16 bytes
        ///         Some(16)
        ///     }
        /// }
        ///
        /// let server = Server::builder()
        ///     .with_connection_id(MyConnectionIdFormat)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_connection_id,
        connection_id,
        ServerProviders
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
        stateless_reset_token,
        ServerProviders
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
        ///     .with_limits(limits::Limits::new().with_max_idle_timeout(Duration::from_secs(40))?)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_limits,
        limits,
        ServerProviders
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
        ///     .with_endpoint_limits(
        ///         endpoint_limits::Default::builder()
        ///             .with_inflight_handshake_limit(100)?
        ///             .build()?,
        ///     )?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_endpoint_limits,
        endpoint_limits,
        ServerProviders
    );

    impl_provider_method!(
        /// Sets the path specific mtu config provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Set custom MTU values to use per path, while inheriting the remaining default
        /// config
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::mtu};
        ///
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        ///
        /// struct MyMtuProvider(mtu::Config);
        ///
        /// impl mtu::Endpoint for MyMtuProvider {
        ///     fn on_path(
        ///         &mut self,
        ///         info: &mtu::PathInfo,
        ///         endpoint_mtu_config: mtu::Config,
        ///     ) -> Option<mtu::Config> {
        ///         Some(self.0)
        ///     }
        /// }
        /// let mtu = MyMtuProvider(mtu::Config::builder().build().unwrap());
        ///
        /// let server = Server::builder()
        ///     .with_mtu(mtu)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_mtu,
        mtu,
        ServerProviders
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
        event,
        ServerProviders
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
        ///     .with_address_token(MyTokenProvider::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_address_token,
        address_token,
        ServerProviders
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
        ///     .with_receive_address(addr)?
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
        io,
        ServerProviders
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
        /// let tls = s2n_tls::Server::builder()
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
        tls,
        ServerProviders
    );

    #[cfg(any(test, feature = "unstable-provider-packet-interceptor"))]
    impl_provider_method!(
        /// Sets the packet interceptor provider for the [`Server`]
        with_packet_interceptor,
        packet_interceptor,
        ServerProviders
    );

    #[cfg(any(test, feature = "unstable-provider-random"))]
    impl_provider_method!(
        /// Sets the random provider for the [`Server`]
        with_random,
        random,
        ServerProviders
    );

    #[cfg(feature = "unstable-provider-datagram")]
    impl_provider_method!(
        /// Sets the datagram provider for the [`Server`]
        with_datagram,
        datagram,
        ServerProviders
    );

    #[cfg(any(test, feature = "unstable-provider-dc"))]
    impl_provider_method!(
        /// Sets the dc provider for the [`Server`]
        with_dc,
        dc,
        ServerProviders
    );

    impl_provider_method!(
        /// Sets the congestion controller provider for the [`Server`]
        with_congestion_controller,
        congestion_controller,
        ServerProviders
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
        self.0.build().start()
    }
}
