use crate::{
    provider::*,
    server::{DefaultProviders, Server, ServerProviders},
};

/// A builder for configuring [`Server`] providers
#[derive(Debug)]
pub struct Builder<Providers>(Providers);

impl Default for Builder<DefaultProviders> {
    fn default() -> Self {
        Self(Default::default())
    }
}

/// An error indicating a failure to build a [`Server`]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuilderError {}

impl<Providers: ServerProviders> Builder<Providers> {
    impl_provider_method!(
        /// Sets the TLS provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// The default TLS provider and configuration will be used with the
        /// path to the private key.
        ///
        /// ```rust
        /// # use std::error::Error;
        /// # use s2n_quic::Server;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_tls("./certs/key.pem")?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Sets the TLS provider to a TLS server builder
        ///
        /// ```rust,ignore
        /// # use std::error::Error;
        /// # use s2n_quic::Server;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let tls = s2n::tls::Server::builder()
        ///     .with_certificate("./certs/key.pem")?
        ///     .with_security_policy(s2n::tls::security_policy::S2N_20190802)?;
        ///
        /// let server = Server::builder()
        ///     .with_tls(tls)?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_tls,
        tls
    );

    impl_provider_method!(
        /// Sets the Congestion Controller provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the congestion controller to `Reno` with the default configuration.
        ///
        /// ```rust
        /// # use std::error::Error;
        /// # use s2n_quic::{Server, provider::congestion_controller::Reno};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_congestion_controller(Reno::default())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_congestion_controller,
        congestion_controller
    );

    impl_provider_method!(
        /// Sets the Socket provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Starts listening on [`127.0.0.1:443`](https://127.0.0.1)
        ///
        /// ```rust
        /// # use std::error::Error;
        /// # use s2n_quic::Server;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_socket("127.0.0.1:443")?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_socket,
        socket
    );

    impl_provider_method!(
        /// Sets the limits provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the max idle time, while inheriting the remaining default limits
        ///
        /// ```rust
        /// # use std::{error::Error, time::Duration};
        /// # use s2n_quic::{Server, provider::limits::Limits};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_limits(
        ///         Limits::builder()
        ///             .with_max_idle_time(Duration::from_secs(30))?
        ///             .build()?
        ///     )?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_limits,
        limits
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
        /// # use s2n_quic::{Server, provider::limits::Limits};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_connection_id(MyConnectionIDFormat::new())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_connection_id,
        connection_id
    );

    impl_provider_method!(
        /// Sets the log provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom logger for the server
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// # use s2n_quic::{Server, provider::limits::Limits};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_log(MyLogger::new("./path/to/logs"))?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_log,
        log
    );

    impl_provider_method!(
        /// Sets the retry token provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets a custom logger for the server
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// # use s2n_quic::{Server, provider::limits::Limits};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_retry_token(MyRetryToken::new())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_retry_token,
        retry_token
    );

    /// Builds the [`Server`] with the configured providers
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::error::Error;
    /// # use s2n_quic::Server;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::builder()
    ///     .with_tls("./certs/key.pem")?
    ///     .with_socket("127.0.0.1:443")?
    ///     .build()?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Result<Server, BuilderError> {
        let _providers = self.0.build();

        // TODO spawn endpoint with providers and return handle

        Ok(Server {})
    }
}
