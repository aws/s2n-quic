// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, Connection},
    provider::*,
};
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_transport::endpoint::handle::Acceptor;

mod builder;
mod providers;

pub use builder::*;
pub use providers::*;

/// A QUIC server endpoint, capable of accepting connections
pub struct Server(Acceptor);

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Server")
            .field("local_addr", &self.local_addr().ok())
            .finish()
    }
}

impl Server {
    /// Starts listening on the provided socket
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// # use s2n_quic::Server;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let server = Server::bind("127.0.0.1:443")?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn bind<T>(socket: T) -> Result<Server, StartError>
    where
        T: io::TryInto,
    {
        let server = Self::builder()
            .with_io(socket)
            .map_err(StartError::new)?
            .start()?;
        Ok(server)
    }

    /// Returns a [`Builder`] which is able to configure the [`Server`] components.
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
    ///     .start()?;
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
    ///     Some(connection) => {
    ///         println!("new connection: {:?}", connection.remote_addr());
    ///     }
    ///     None => println!("server closed"),
    /// }
    /// #    Ok(())
    /// # }
    /// ```
    pub async fn accept(&mut self) -> Option<Connection> {
        futures::future::poll_fn(|cx| self.poll_accept(cx)).await
    }

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn poll_accept(&mut self, cx: &mut Context) -> Poll<Option<Connection>> {
        match self.0.poll_accept(cx) {
            Poll::Ready(Some(connection)) => Poll::Ready(Some(Connection::new(connection))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Attempt to close the server endpoint
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
    ///     Some(connection) => {
    ///         println!("new connection: {:?}", connection.remote_addr());
    ///     }
    ///     None => println!("server closed"),
    /// }
    ///
    /// server.close().await?;
    /// #    Ok(())
    /// # }
    ///
    /// ```
    pub async fn close(&self) -> Result<(), connection::Error> {
        futures::future::poll_fn(|cx| self.0.poll_close(cx)).await
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
        // TODO: Return the actual local address
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

impl futures::stream::Stream for Server {
    type Item = Connection;

    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        self.poll_accept(cx)
    }
}
