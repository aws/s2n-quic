// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
        ) -> $crate::connection::Result<$crate::stream::LocalStream> {
            futures::future::poll_fn(|cx| self.poll_open_stream(stream_type, cx)).await
        }

        /// Polls opening a stream with a specific type
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_stream(
            &mut self,
            stream_type: $crate::stream::Type,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::LocalStream>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::{BidirectionalStream, SendStream};

            Ok(
                match futures::ready!(self.0.poll_open_stream(stream_type, cx))? {
                    stream if stream_type == StreamType::Unidirectional => {
                        SendStream::new(stream.into()).into()
                    }
                    stream => BidirectionalStream::new(stream).into(),
                },
            )
            .into()
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
        ) -> $crate::connection::Result<$crate::stream::BidirectionalStream> {
            futures::future::poll_fn(|cx| self.poll_open_bidirectional_stream(cx)).await
        }

        /// Polls opening a bidirectional stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_bidirectional_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::BidirectionalStream>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::BidirectionalStream;

            let stream = futures::ready!(self.0.poll_open_stream(StreamType::Bidirectional, cx))?;

            Ok(BidirectionalStream::new(stream)).into()
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
        ) -> $crate::connection::Result<$crate::stream::SendStream> {
            futures::future::poll_fn(|cx| self.poll_open_send_stream(cx)).await
        }

        /// Polls opening a send stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_open_send_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::SendStream>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::SendStream;

            let stream = futures::ready!(self.0.poll_open_stream(StreamType::Unidirectional, cx))?;

            Ok(SendStream::new(stream.into())).into()
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
        pub fn sni(&self) -> Option<::bytes::Bytes> {
            self.0.sni()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn alpn(&self) -> ::bytes::Bytes {
            self.0.alpn()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn ping(&mut self) -> $crate::connection::Result<()> {
            self.0.ping()
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

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn close(self, error_code: $crate::application::Error) {
            self.0.close(error_code)
        }
    };
}

#[derive(Clone, Debug)]
pub struct Handle(pub(crate) s2n_quic_transport::connection::Connection);

impl Handle {
    impl_handle_api!(|handle, call| call!(handle));
}
