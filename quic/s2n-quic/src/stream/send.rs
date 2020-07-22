/// A QUIC stream that is only allowed to send data.
///
/// The [`SendStream`] implements the required send operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct SendStream {
    // TODO
}

macro_rules! impl_send_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Pushes data onto the stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn push(&mut self, data: bytes::Bytes) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable)
                };
                ($variant: expr) => {
                    futures::sink::SinkExt::send($variant, data).await
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Polls send availability status of the stream.
        ///
        /// This method _must_ be called before calling `Self::send_data`.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_ready(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            let _ = cx;
            todo!()
        }

        /// Sends data on the stream.
        ///
        /// `Self::poll_ready` _must_ be called before calling this method.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn send_data(&mut self, data: bytes::Bytes) -> $crate::stream::Result<()> {
            let _ = data;
            todo!()
        }

        /// Finishes and closes the stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn finish(&mut self) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable)
                };
                ($variant: expr) => {
                    futures::sink::SinkExt::close($variant).await
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Polls finishing and closing the stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_finish(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            let _ = cx;
            todo!()
        }

        /// Initiates a `RESET` of the `Stream`
        ///
        /// This will trigger sending a `RESET` message to the peer, which will
        /// contain the given error code.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn reset(
            &mut self,
            error_code: $crate::ApplicationErrorCode,
        ) -> $crate::stream::Result<()> {
            let _ = error_code;
            todo!()
        }
    };
}

macro_rules! impl_send_stream_trait {
    ($name:ident, | $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        impl futures::sink::Sink<bytes::Bytes> for $name {
            type Error = $crate::stream::Error;

            fn poll_ready(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                let _ = cx;
                todo!()
            }

            fn start_send(
                self: core::pin::Pin<&mut Self>,
                cx: bytes::Bytes,
            ) -> $crate::stream::Result<()> {
                let _ = cx;
                todo!()
            }

            fn poll_flush(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                let _ = cx;
                todo!()
            }

            fn poll_close(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                let _ = cx;
                todo!()
            }
        }

        #[cfg(feature = "std")]
        impl futures::io::AsyncWrite for $name {
            fn poll_write(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &[u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                let _ = cx;
                let _ = buf;
                todo!()
            }

            fn poll_flush(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                let _ = cx;
                todo!()
            }

            fn poll_close(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                let _ = cx;
                todo!()
            }
        }
    };
}

impl SendStream {
    impl_send_stream_api!(|stream, dispatch| dispatch!(stream));

    impl_splittable_stream_api!(|stream| (None, Some(stream)));

    impl_connection_api!(|_stream| todo!());

    impl_metric_api!();
}

impl_splittable_stream_trait!(SendStream, |stream| (None, Some(stream)));
impl_send_stream_trait!(SendStream, |stream, dispatch| dispatch!(stream));
