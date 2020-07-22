/// A QUIC stream that is only allowed to receive data.
///
/// The [`ReceiveStream`] implements the required operations receive described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct ReceiveStream {
    // TODO
}

macro_rules! impl_receive_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Reads the next chunk of data from the [`Stream`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn pop(&mut self) -> $crate::stream::Result<Option<bytes::Bytes>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonReadable)
                };
                ($variant: expr) => {
                    futures::stream::TryStreamExt::try_next($variant).await
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Poll for more data received from the remote on this stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_data(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<Option<bytes::Bytes>>> {
            // macro_rules! $dispatch {
            //     () => {
            //         core::task::Poll::Ready(Err($crate::stream::StreamError::NonReadable))
            //     };
            //     ($variant: expr) => {
            //         $variant.poll_data(cx)
            //     };
            // }

            // let $stream = self;
            // $dispatch_body
            let _ = cx;
            todo!()
        }

        /// Sends a `STOP_SENDING` message to the peer. This requests the peer to
        /// finish the `Stream` as soon as possible by issuing a `RESET` with the
        /// provided `error_code`.
        ///
        /// Since this is merely a request to the peer to `RESET` the `Stream`, the
        /// `Stream` will not immediately be in a `RESET` state after issuing this
        /// API call.
        ///
        /// If the `Stream` had been previously reset by the peer or if all data had
        /// already been received the API call will not trigger any action.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn stop_sending(
            &mut self,
            error_code: $crate::ApplicationErrorCode,
        ) -> $crate::stream::Result<()> {
            let _ = error_code;
            todo!()
        }
    };
}

macro_rules! impl_receive_stream_trait {
    ($name:ident, | $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        impl futures::stream::Stream for $name {
            type Item = $crate::stream::Result<bytes::Bytes>;

            fn poll_next(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Option<Self::Item>> {
                let _ = cx;
                todo!()
            }
        }

        #[cfg(feature = "std")]
        impl futures::io::AsyncRead for $name {
            fn poll_read(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &mut [u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                let _ = cx;
                let _ = buf;
                todo!()
            }
        }
    };
}

impl ReceiveStream {
    impl_receive_stream_api!(|stream, dispatch| dispatch!(stream));

    impl_splittable_stream_api!(|stream| (Some(stream), None));

    impl_connection_api!(|_stream| todo!());

    impl_metric_api!();
}

impl_splittable_stream_trait!(ReceiveStream, |stream| (None, Some(stream)));
impl_receive_stream_trait!(ReceiveStream, |stream, dispatch| dispatch!(stream));
