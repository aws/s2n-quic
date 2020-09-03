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
#[derive(Debug)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
#[non_exhaustive]
pub enum BuilderError {}

impl<Providers: ServerProviders> Builder<Providers> {
    impl_provider_method!(
        /// Sets the clock provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// # use std::error::Error;
        /// use s2n_quic::{Server, provider::clock};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_clock(clock::Default::default())?
        ///     .build()?;
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        with_clock,
        clock
    );

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
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_congestion_controller(congestion_controller::Reno::default())?
        ///     .build()?;
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
        /// Sets the limits provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Sets the max idle time, while inheriting the remaining default limits
        ///
        /// ```rust
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::limits};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_limits(limits::Default::default())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_limits,
        limits
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
        /// use s2n_quic::Server;
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
        /// use s2n_quic::Server;
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

    impl_provider_method!(
        /// Sets the runtime provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Uses the tokio runtime.
        ///
        /// ```rust
        /// # use std::error::Error;
        /// use s2n_quic::{Server, provider::runtime};
        /// #
        /// # #[tokio::main]
        /// # async fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_runtime(tokio::runtime::Handle::current())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_runtime,
        runtime
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
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_io("127.0.0.1:443")?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        ///
        /// Configures a socket pair
        ///
        /// ```rust,no_run
        /// # use std::error::Error;
        /// use s2n_quic::{Server, provider::io::platform as io};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let addr = "127.0.0.1:443";
        ///
        /// let socket = io::Socket::builder()?
        ///     .with_address(addr)?
        ///     .with_address_reuse()?
        ///     .build()?;
        ///
        /// let rx = io::Rx::new(io::Buffer::default(), socket.try_clone()?);
        /// let tx = io::Tx::new(io::Buffer::default(), socket);
        /// let duplex = io::Duplex { rx, tx };
        ///
        /// let server = Server::builder()
        ///     .with_io(duplex)?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_io,
        io
    );

    impl_provider_method!(
        /// Sets the synchronization provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// Uses [`std::sync::Mutex`] to perform synchronization.
        ///
        /// ```rust,ignore
        /// # use std::{error::Error, time::Duration};
        /// use s2n_quic::{Server, provider::sync};
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_sync(sync::Mutex::default())?
        ///     .build()?;
        /// #
        /// #    Ok(())
        /// # }
        /// ```
        with_sync,
        sync
    );

    impl_provider_method!(
        /// Sets the TLS provider for the [`Server`]
        ///
        /// # Examples
        ///
        /// The default TLS provider and configuration will be used with the
        /// path to the private key.
        ///
        /// ```rust
        /// # use std::{error::Error, path::Path};
        /// # use s2n_quic::Server;
        /// #
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let server = Server::builder()
        ///     .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
        ///     .build()?;
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
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// let tls = s2n::tls::Server::builder()
        ///     .with_certificate(Path::new("./certs/cert.pem"), Path::new("./certs/key.pem"))?
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

    /// Builds the [`Server`] with the configured providers
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::{error::Error, path::Path};
    /// # use s2n_quic::Server;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::builder()
    ///     .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
    ///     .with_io("127.0.0.1:443")?
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
