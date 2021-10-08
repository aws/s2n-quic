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
use s2n_quic_transport::endpoint::{connect, Handle};

mod builder;
mod providers;

pub use builder::*;
pub use connect::Connect;
pub use providers::*;

/// A QUIC client endpoint, capable of opening connections
pub struct Client {
    handle: Handle,
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
    /// # use std::{error::Error, path::Path};
    /// # use s2n_quic::Client;
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

    /// TODO
    pub fn connect(&self, connect: Connect) -> ConnectionAttempt {
        let attempt = self.handle.connect(connect);
        ConnectionAttempt(attempt)
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
    #[cfg(feature = "std")]
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        // TODO: Return the actual local address
        Ok("127.0.0.1:12345".parse().unwrap())
    }
}

pub struct ConnectionAttempt(connect::Attempt);

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
