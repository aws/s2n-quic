use crate::{
    connection::{self, Connection},
    provider::*,
};
use core::task::{Context, Poll};

mod builder;
pub mod metric;

pub use builder::*;
pub use metric::Metric;

/// A QUIC server endpoint, capable of accepting connections
#[derive(Debug)]
pub struct Server {
    // TODO
}

impl_providers_state! {
    #[derive(Debug, Default)]
    struct Providers {
        clock: Clock,
        congestion_controller: CongestionController,
        connection_id: ConnectionID,
        limits: Limits,
        log: Log,
        retry_token: RetryToken,
        runtime: Runtime,
        socket: Socket,
        sync: Sync,
        tls: Tls,
    }

    /// Opaque trait containing all of the configured providers
    trait ServerProviders {}
}

impl Server {
    /// Starts listening on the provided socket
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::error::Error;
    /// # use s2n_quic::Server;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::bind("127.0.0.1:443")?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn bind<T>(socket: T) -> Result<Server, T::Error>
    where
        T: socket::TryInto,
    {
        let server = Self::builder()
            .with_socket(socket)?
            .build()
            .expect("The server could not be built");
        Ok(server)
    }

    /// Returns a [`Builder`] which is able to configure the [`Server`] components.
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
    pub fn builder() -> Builder<impl ServerProviders> {
        Builder::default()
    }

    /// Accepts a new incoming [`Connection`] from this [`Server`].
    ///
    /// This function will yield once a new QUIC connection is established. When established,
    /// the corresponding [`Connection`] and the remote peer's address will be returned.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// # use s2n_quic::Server;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let mut server = Server::bind("127.0.0.1:443")?;
    ///
    /// match server.accept().await {
    ///     Ok(connection) => {
    ///         println!("new connection: {:?}", connection.remote_addr());
    ///     }
    ///     Err(e) => println!("connection failed: {:?}", e),
    /// }
    /// #    Ok(())
    /// # }
    /// ```
    pub async fn accept(&mut self) -> connection::Result<Connection> {
        todo!()
    }

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn poll_accept(&mut self, cx: &mut Context) -> Poll<connection::Result<Connection>> {
        let _ = cx;
        todo!()
    }

    /// Returns the local address that this listener is bound to.
    ///
    /// This can be useful, for example, when binding to port `0` to figure out which
    /// port was actually bound.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// # use s2n_quic::Server;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::bind("127.0.0.1:443")?;
    ///
    /// let local_addr = server.local_addr()?;
    ///
    /// #    Ok(())
    /// # }
    /// ```
    #[cfg(feature = "std")]
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        todo!()
    }

    /// Returns the current value of the given metric
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn query<Metric: self::Metric>(&self) -> connection::Result<Metric> {
        todo!()
    }
}

impl futures::stream::Stream for Server {
    type Item = connection::Result<Connection>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let _ = cx;
        todo!()
    }
}
