// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::{Client, ClientProviders, DefaultProviders},
    provider::*,
};

/// A builder for configuring [`Client`] providers
#[derive(Debug)]
pub struct Builder<Providers>(pub(crate) Providers);

impl Default for Builder<DefaultProviders> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Providers: ClientProviders> Builder<Providers> {
    impl_provider_method!(
        /// Sets the connection ID provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Uses the default connection ID provider with the default configuration.
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Client, provider::connection_id};
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_connection_id(connection_id::Default::default())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Sets a custom connection ID provider for the client
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Client, provider::connection_id};
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
        /// let client = Client::builder()
        ///     .with_connection_id(MyConnectionIdFormat)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_connection_id,
        connection_id,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the stateless reset token provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Sets a custom stateless reset token provider for the client
        ///
        /// ```rust,ignore
        /// # use std::error::Error;
        /// use s2n_quic::Client;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_stateless_reset_token(MyStatelessResetTokenGenerator::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_stateless_reset_token,
        stateless_reset_token,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the limits provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Sets the max idle time, while inheriting the remaining default limits
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Client, provider::limits};
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_limits(limits::Limits::new().with_max_idle_timeout(Duration::from_secs(40))?)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_limits,
        limits,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the path specific mtu config provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Set custom MTU values to use per path, while inheriting the remaining default
        /// config
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Client, provider::mtu};
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
        /// let client = Client::builder()
        ///     .with_mtu(mtu)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_mtu,
        mtu,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the event provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Sets a custom event subscriber for the client
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::Client;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_event(MyEventLogger::new())?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_event,
        event,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the IO provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// Opens a socket on an address chosen by the operating system
        ///
        /// ```rust,no_run
        /// # use std::error::Error;
        /// use s2n_quic::Client;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_io("0.0.0.0:0")?
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
        /// use s2n_quic::{Client, provider::io::tokio::Builder as IoBuilder};
        /// use std::net::ToSocketAddrs;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let addr = "0.0.0.0:0".to_socket_addrs()?.next().unwrap();
        ///
        /// let io = IoBuilder::default()
        ///     .with_receive_address(addr)?
        ///     .build()?;
        ///
        /// let client = Client::builder()
        ///     .with_io(io)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_io,
        io,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the TLS provider for the [`Client`]
        ///
        /// # Examples
        ///
        /// The default TLS provider and configuration will be used with the
        /// path to the private key.
        ///
        /// ```rust,no_run
        /// # use std::{error::Error, path::Path};
        /// # use s2n_quic::Client;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let client = Client::builder()
        ///     .with_tls(Path::new("./certs/cert.pem"))?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Sets the TLS provider to a TLS client builder
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, path::Path};
        /// # use s2n_quic::Client;
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let tls = s2n_tls::Client::builder()
        ///     .with_certificate(Path::new("./certs/cert.pem"))?
        ///     .with_security_policy(s2n::tls::security_policy::S2N_20190802)?;
        ///
        /// let client = Client::builder()
        ///     .with_tls(tls)?
        ///     .start()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_tls,
        tls,
        ClientProviders
    );

    #[cfg(any(test, feature = "unstable-provider-packet-interceptor"))]
    impl_provider_method!(
        /// Sets the packet interceptor provider for the [`Client`]
        with_packet_interceptor,
        packet_interceptor,
        ClientProviders
    );

    #[cfg(any(test, feature = "unstable-provider-random"))]
    impl_provider_method!(
        /// Sets the random provider for the [`Client`]
        with_random,
        random,
        ClientProviders
    );

    #[cfg(feature = "unstable-provider-datagram")]
    impl_provider_method!(
        /// Sets the datagram provider for the [`Client`]
        with_datagram,
        datagram,
        ClientProviders
    );

    #[cfg(any(test, feature = "unstable-provider-dc"))]
    impl_provider_method!(
        /// Sets the dc provider for the [`Client`]
        with_dc,
        dc,
        ClientProviders
    );

    impl_provider_method!(
        /// Sets the congestion controller provider for the [`Client`]
        with_congestion_controller,
        congestion_controller,
        ClientProviders
    );

    /// Starts the [`Client`] with the configured providers
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::{error::Error, path::Path};
    /// # use s2n_quic::Client;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let client = Client::builder()
    ///     .with_tls(Path::new("./certs/cert.pem"))?
    ///     .with_io("0.0.0.0:0")?
    ///     .start()?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn start(self) -> Result<Client, StartError> {
        self.0.build().start()
    }
}
