macro_rules! impl_handle_api {
    (| $handle:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Opens a new stream with a specific type
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn open_stream(
            &mut self,
            stream_type: $crate::stream::Type,
        ) -> $crate::stream::Result<$crate::stream::LocalStream> {
            let _ = stream_type;
            todo!()
        }

        /// Polls opening a stream with a specific type
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_stream(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context,
            stream: $crate::stream::Type,
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::LocalStream>> {
            let _ = cx;
            let _ = stream;
            todo!()
        }

        /// Opens a bidirectional stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn open_bidirectional_stream(
            &mut self,
        ) -> $crate::stream::Result<$crate::stream::BidirectionalStream> {
            todo!();
        }

        /// Polls opening a bidirectional stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_bidirectional_stream(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::BidirectionalStream>> {
            let _ = cx;
            todo!()
        }

        /// Opens a send stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn open_send_stream(
            &mut self,
        ) -> $crate::stream::Result<$crate::stream::SendStream> {
            todo!();
        }

        /// Polls opening a send stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_send_stream(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::SendStream>> {
            let _ = cx;
            todo!()
        }

        /// Returns the local address that this connection is bound to.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[cfg(feature = "std")]
        pub fn local_addr(&self) -> $crate::connection::Result<std::net::SocketAddr> {
            todo!()
        }

        /// Returns the remote address that this connection is connected to.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[cfg(feature = "std")]
        pub fn remote_addr(&self) -> $crate::connection::Result<std::net::SocketAddr> {
            todo!()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn handshake_status(
            &self,
        ) -> $crate::connection::Result<$crate::connection::HandshakeStatus> {
            todo!()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn ping(&mut self) -> $crate::connection::Result<()> {
            todo!()
        }

        /// Returns a reference to the [`Extensions`] for the [`Connection`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[cfg(feature = "protocol-extensions")]
        pub fn extensions(&self) -> $crate::connection::Result<&crate::extensions::Extensions> {
            todo!()
        }

        /// Returns a mutable reference to the [`Extensions`] for the [`Connection`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[cfg(feature = "protocol-extensions")]
        pub fn extensions_mut(
            &mut self,
        ) -> $crate::connection::Result<&mut crate::extensions::Extensions> {
            todo!()
        }

        /// Returns the current value of the given metric
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn query<Metric: $crate::connection::Metric>(
            &self,
        ) -> $crate::connection::Result<Metric> {
            todo!()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn close(
            self,
            error_code: $crate::ApplicationErrorCode,
        ) -> $crate::connection::Result<()> {
            let _ = error_code;
            todo!()
        }
    };
}

#[derive(Clone, Debug)]
pub struct Handle {
    // TODO
}

impl Handle {
    impl_handle_api!(|handle, call| call!(handle));
}
