// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_quic_transport::connection::Connection as Inner;

#[macro_use]
mod acceptor;
#[macro_use]
mod handle;

pub use acceptor::*;
pub use handle::*;
pub use s2n_quic_core::connection::Error;

pub type Result<T, E = Error> = core::result::Result<T, E>;

pub struct Connection(Inner);

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Connection {
    #[inline]
    pub(crate) const fn new(inner: Inner) -> Self {
        Self(inner)
    }

    impl_accept_api!();
    impl_handle_api!(|handle, call| call!(handle));

    /// Returns a cloneable handle to the connection
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn test() {
    /// #   let mut connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// let handle = connection.handle();
    /// let another_handle = handle.clone();
    /// # }
    /// ```
    #[inline]
    pub fn handle(&self) -> Handle {
        Handle(self.0.clone())
    }

    /// Splits the connection into a  [`connection::Handle`](crate::connection::Handle) and
    /// [`connection::StreamAcceptor`](crate::connection::StreamAcceptor) halves
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn test() -> s2n_quic::connection::Result<()> {
    /// #   let mut connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// let (mut handle, mut acceptor) = connection.split();
    /// let send = handle.open_send_stream()?;
    /// tokio::spawn(async move {
    ///     let _ = send.send(Bytes::from_static(&[1, 2, 3])).await;
    /// });
    ///
    /// while let Some(stream) = acceptor.accept()? {
    ///     println!("accepted stream {}", stream.id());
    /// }
    ///
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn split(self) -> (Handle, StreamAcceptor) {
        let handle = Handle(self.0.clone());
        let acceptor = StreamAcceptor(self.0);
        (handle, acceptor)
    }
}
