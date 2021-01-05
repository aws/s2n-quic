use s2n_quic_transport::stream;

/// A QUIC stream that is only allowed to receive data.
///
/// The [`ReceiveStream`] implements the required operations receive described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct ReceiveStream(stream::ReceiveStream);

macro_rules! impl_receive_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Reads the next chunk of data from the [`Stream`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn receive(&mut self) -> $crate::stream::Result<Option<bytes::Bytes>> {
            ::futures::future::poll_fn(|cx| self.poll_receive(cx)).await
        }

        /// Poll for more data received from the remote on this stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_receive(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<Option<bytes::Bytes>>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonReadable).into()
                };
                ($variant: expr) => {
                    $variant.poll_receive(cx)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Reads the next slice of chunked data from the [`Stream`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn receive_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
        ) -> $crate::stream::Result<(usize, bool)> {
            ::futures::future::poll_fn(|cx| self.poll_receive_vectored(chunks, cx)).await
        }

        /// Poll for more data received from the remote on this stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_receive_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<(usize, bool)>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonReadable).into()
                };
                ($variant: expr) => {
                    $variant.poll_receive_vectored(chunks, cx)
                };
            }

            let $stream = self;
            $dispatch_body
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
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonReadable)
                };
                ($variant: expr) => {
                    $variant.stop_sending(error_code)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Create a batch request for receiving data
        pub(crate) fn rx_request(
            &mut self,
        ) -> $crate::stream::Result<s2n_quic_transport::stream::RxRequest> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonReadable)
                };
                ($variant: expr) => {
                    $variant.rx_request()
                };
            }

            let $stream = self;
            $dispatch_body
        }
    };
}

macro_rules! impl_receive_stream_trait {
    ($name:ident, | $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        impl futures::stream::Stream for $name {
            type Item = $crate::stream::Result<bytes::Bytes>;

            fn poll_next(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Option<Self::Item>> {
                match futures::ready!(self.poll_receive(cx)) {
                    Ok(Some(v)) => Some(Ok(v)),
                    Ok(None) => None,
                    Err(err) => Some(Err(err)),
                }
                .into()
            }
        }

        #[cfg(feature = "std")]
        impl futures::io::AsyncRead for $name {
            fn poll_read(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &mut [u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                use bytes::Bytes;

                if buf.is_empty() {
                    return Ok(0).into();
                }

                // create some chunks on the stack to receive into
                // TODO investigate a better default number
                let mut chunks = [
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                ];

                let high_watermark = buf.len();

                let response = futures::ready!(self
                    .rx_request()?
                    .receive(&mut chunks)
                    // don't receive more than we're capable of storing
                    .with_high_watermark(high_watermark)
                    .poll(Some(cx))?
                    .into_poll());

                let chunks = &chunks[..response.chunks.consumed];
                let mut bufs = [buf];
                let copied_len = s2n_quic_core::slice::vectored_copy(chunks, &mut bufs);

                debug_assert_eq!(
                    copied_len, response.bytes.consumed,
                    "the consumed bytes should always have enough capacity in bufs"
                );

                Ok(response.bytes.consumed).into()
            }

            fn poll_read_vectored(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                bufs: &mut [futures::io::IoSliceMut],
            ) -> core::task::Poll<std::io::Result<usize>> {
                use bytes::Bytes;

                if bufs.is_empty() {
                    return Ok(0).into();
                }

                // create some chunks on the stack to receive into
                // TODO investigate a better default number
                let mut chunks = [
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                    Bytes::new(),
                ];

                let high_watermark = bufs.iter().map(|buf| buf.len()).sum();

                let response = futures::ready!(self
                    .rx_request()?
                    .receive(&mut chunks)
                    // don't receive more than we're capable of storing
                    .with_high_watermark(high_watermark)
                    .poll(Some(cx))?
                    .into_poll());

                let chunks = &chunks[..response.chunks.consumed];
                let copied_len = s2n_quic_core::slice::vectored_copy(chunks, bufs);

                debug_assert_eq!(
                    copied_len, response.bytes.consumed,
                    "the consumed bytes should always have enough capacity in bufs"
                );

                Ok(copied_len).into()
            }
        }

        #[cfg(all(feature = "std", feature = "tokio"))]
        impl tokio::io::AsyncRead for $name {
            fn poll_read(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &mut [u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                futures::io::AsyncRead::poll_read(self, cx, buf)
            }
        }
    };
}

impl ReceiveStream {
    pub(crate) const fn new(stream: stream::ReceiveStream) -> Self {
        Self(stream)
    }

    impl_receive_stream_api!(|stream, dispatch| dispatch!(stream.0));

    impl_connection_api!(|_stream| todo!());
}

impl_splittable_stream_trait!(ReceiveStream, |stream| (Some(stream), None));
impl_receive_stream_trait!(ReceiveStream, |stream, dispatch| dispatch!(stream.0));
