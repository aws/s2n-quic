// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, Connection},
    provider::*,
};
use core::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_transport::endpoint::{connect, handle::Connector};

mod builder;
mod providers;

pub use builder::*;
pub use connect::Connect;
pub use providers::*;

/// A QUIC client endpoint, capable of opening connections
#[derive(Clone)]
pub struct Client {
    connector: Connector,
    local_addr: s2n_quic_core::inet::SocketAddress,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("local_addr", &self.local_addr().ok())
            .finish()
    }
}

impl Client {
    /// Starts listening on the provided socket
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// # use s2n_quic::Client;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let Client = Client::bind("0.0.0.0:0")?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn bind<T>(socket: T) -> Result<Self, StartError>
    where
        T: io::TryInto,
    {
        let client = Self::builder()
            .with_io(socket)
            .map_err(StartError::new)?
            .start()?;
        Ok(client)
    }

    /// Returns a [`Builder`] which is able to configure the [`Client`] components.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// use std::path::Path;
    /// use s2n_quic::Client;
    ///
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let client = Client::builder()
    ///     .with_tls(Path::new("./certs/cert.pem"))?
    ///     .with_io("0.0.0.0:0")?
    ///     .start()?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn builder() -> Builder<impl ClientProviders> {
        Builder::default()
    }

    /// Establishes a connection to the specified endpoint
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// use s2n_quic::Client;
    /// use std::{net::SocketAddr, path::Path};
    ///
    /// # async fn connect() -> Result<(), Box<dyn Error>> {
    /// let client = Client::builder()
    ///     .with_tls(Path::new("./certs/cert.pem"))?
    ///     .with_io("0.0.0.0:0")?
    ///     .start()?;
    ///
    /// let addr: SocketAddr = "127.0.0.1:443".parse()?;
    /// let connection = client.connect(addr.into()).await?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub fn connect(&self, connect: Connect) -> ConnectionAttempt {
        let attempt = self.connector.connect(connect);
        ConnectionAttempt(attempt)
    }

    /// Wait for the client endpoint to finish handling all outstanding connections
    ///
    /// Notifies the endpoint of application interest in closing the endpoint. The
    /// call waits for **all** outstanding connections to finish before returning.
    ///
    /// Note: The endpoint will continue to accept new connection attempts. If there
    /// are other client handles with active connections, then this call will never
    /// return.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::error::Error;
    /// use s2n_quic::Client;
    /// use std::{net::SocketAddr, path::Path};
    ///
    /// # async fn connect() -> Result<(), Box<dyn Error>> {
    /// let mut client = Client::builder()
    ///     .with_tls(Path::new("./certs/cert.pem"))?
    ///     .with_io("0.0.0.0:0")?
    ///     .start()?;
    ///
    /// let addr: SocketAddr = "127.0.0.1:443".parse()?;
    /// let connection = client.connect(addr.into()).await?;
    ///
    /// client.wait_idle().await?;
    /// #
    /// #    Ok(())
    /// # }
    /// ```
    pub async fn wait_idle(&mut self) -> Result<(), connection::Error> {
        futures::future::poll_fn(|cx| self.connector.poll_close(cx)).await
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
    /// # use s2n_quic::Client;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let client = Client::bind("0.0.0.0:0")?;
    ///
    /// let local_addr = client.local_addr()?;
    /// assert_ne!(local_addr.port(), 0);
    /// #    Ok(())
    /// # }
    /// ```
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        Ok(self.local_addr.into())
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ConnectionAttempt(connect::Attempt);

impl ConnectionAttempt {
    /// Configure application context on this connection attempt.
    ///
    /// This context is passed to the `Subscriber::create_connection_context` method, allowing the
    /// application to thread arbitrary connection-specific context into the subscriber
    /// implementation.
    pub fn set_application_context(&mut self, application: Box<dyn std::any::Any + Send + Sync>) {
        self.0.set_application_context(application);
    }
}

impl Future for ConnectionAttempt {
    type Output = Result<Connection, connection::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(Ok(conn)) => Poll::Ready(Ok(Connection::new(conn))),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
