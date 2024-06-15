// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Connection, provider::*};
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_transport::endpoint::handle::Acceptor;

mod builder;
mod providers;

pub use builder::*;
pub use providers::*;
pub use s2n_quic_core::application::ServerName as Name;

/// A QUIC server endpoint, capable of accepting connections
pub struct Server {
    acceptor: Acceptor,
    local_addr: s2n_quic_core::inet::SocketAddress,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Server")
            .field("local_addr", &self.local_addr().ok())
            .finish()
    }
}

impl Server {
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
    /// the corresponding [`Connection`] will be returned.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::{error::Error, path::Path};
    /// # use s2n_quic::Server;
    /// #
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let mut server = Server::builder()
    ///     .with_tls((Path::new("./certs/cert.pem"), Path::new("./certs/key.pem")))?
    ///     .with_io("127.0.0.1:443")?
    ///     .start()?;
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

    /// Attempts to accept a new incoming [`Connection`] from this [`Server`].
    ///
    /// # Return value
    ///
    /// This function returns:
    ///
    /// - `Poll::Pending` if no new connections have been established.
    /// - `Poll::Ready(Some(connection))` once a new connection has been established.
    ///   This function can be called again to try and accept new connections.
    /// - `Poll::Ready(None)` the attempt failed because the server has closed. Once
    ///   None is returned, this function should not be called again.
    pub fn poll_accept(&mut self, cx: &mut Context) -> Poll<Option<Connection>> {
        s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
            match self.acceptor.poll_accept(cx) {
                Poll::Ready(Some(connection)) => Poll::Ready(Some(Connection::new(connection))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        })
    }

    /// Returns the local address that this listener is bound to.
    ///
    /// This can be useful, for example, when binding to port `0` to figure out which
    /// port was actually bound.
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
    ///
    /// let local_addr = server.local_addr()?;
    ///
    /// #    Ok(())
    /// # }
    /// ```
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        Ok(self.local_addr.into())
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
