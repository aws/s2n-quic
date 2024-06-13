// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_transport::stream;

/// A QUIC stream that is only allowed to receive data.
#[derive(Debug)]
pub struct ReceiveStream(stream::ReceiveStream);

macro_rules! impl_receive_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Receives a chunk of data from the stream.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Ok(Some(chunk))` if the stream is open and data was available.
        /// - `Ok(None)` if the stream was finished and all of the data was consumed.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut stream: s2n_quic::stream::ReceiveStream = todo!();
        /// #
        /// while let Some(chunk) = stream.receive().await? {
        ///     println!("received: {:?}", chunk);
        /// }
        ///
        /// println!("finished");
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn receive(&mut self) -> $crate::stream::Result<Option<bytes::Bytes>> {
            ::futures::future::poll_fn(|cx| self.poll_receive(cx)).await
        }

        /// Poll for a chunk of data from the stream.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Poll::Pending` if the stream is waiting to receive data from the peer. In this case,
        ///   the caller should retry receiving after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(Some(chunk)))` if the stream is open and data was available.
        /// - `Poll::Ready(Ok(None))` if the stream was finished and all of the data was consumed.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_receive(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<Option<bytes::Bytes>>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_readable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::contract_debug(cx, |cx| $variant.poll_receive(cx))
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Receives a slice of chunks of data from the stream.
        ///
        /// This can be more efficient than calling [`receive`](Self::receive) for each chunk,
        /// especially when receiving large amounts of data.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Ok((count, is_open))` if the stream received data into the slice,
        ///   where `count` was the number of chunks received, and `is_open` indicating if the stream is
        ///   still open. If `is_open == true`, `count` will be at least `1`. If `is_open == false`, future calls to
        ///   [`receive_vectored`](Self::receive_vectored) will always return
        ///   `Ok((0, false))`.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut stream: s2n_quic::stream::ReceiveStream = todo!();
        /// #
        /// # use bytes::Bytes;
        /// #
        /// loop {
        ///     let mut chunks = [Bytes::new(), Bytes::new(), Bytes::new()];
        ///     let (count, is_open) = stream.receive_vectored(&mut chunks).await?;
        ///
        ///     for chunk in &chunks[..count] {
        ///         println!("received: {:?}", chunk);
        ///     }
        ///
        ///     if !is_open {
        ///         break;
        ///     }
        /// }
        ///
        /// println!("finished");
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn receive_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
        ) -> $crate::stream::Result<(usize, bool)> {
            ::futures::future::poll_fn(|cx| self.poll_receive_vectored(chunks, cx)).await
        }

        /// Polls for receiving a slice of chunks of data from the stream.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Poll::Pending` if the stream is waiting to receive data from the peer. In this case,
        ///   the caller should retry receiving after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok((count, is_open)))` if the stream received data into the slice,
        ///   where `count` was the number of chunks received, and `is_open` indicating if the stream is
        ///   still open. If `is_open == true`, `count` will be at least `1`. If `is_open == false`, future calls to
        ///   [`poll_receive_vectored`](Self::poll_receive_vectored) will always return
        ///   `Poll::Ready(Ok((0, false)))`.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_receive_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<(usize, bool)>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_readable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::contract_debug(cx, |cx| {
                        $variant.poll_receive_vectored(chunks, cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Notifies the peer to stop sending data on the stream.
        ///
        /// This requests the peer to finish the stream as soon as possible
        /// by issuing a [`reset`](crate::stream::SendStream::reset) with the
        /// provided [`error_code`](crate::application::Error).
        ///
        /// Since this is merely a request for the peer to reset the stream, the
        /// stream will not immediately be in a reset state after issuing this
        /// call.
        ///
        /// If the stream has already been reset by the peer or if all data has
        /// been received, the call will not trigger any action.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Ok(())` if the stop sending message was enqueued for the peer.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut connection: s2n_quic::connection::Connection = todo!();
        /// #
        /// while let Some(stream) = connection.accept_receive_stream().await? {
        ///     stream.stop_sending(123u8.into());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub fn stop_sending(
            &mut self,
            error_code: $crate::application::Error,
        ) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_readable())
                };
                ($variant: expr) => {
                    $variant.stop_sending(error_code)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Create a batch request for receiving data
        #[inline]
        pub(crate) fn rx_request(
            &mut self,
        ) -> $crate::stream::Result<s2n_quic_transport::stream::RxRequest> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_readable())
                };
                ($variant: expr) => {
                    $variant.rx_request()
                };
            }

            let $stream = self;
            $dispatch_body
        }

        #[inline]
        pub(crate) fn receive_chunks(
            &mut self,
            cx: &mut core::task::Context,
            chunks: &mut [bytes::Bytes],
            high_watermark: usize,
        ) -> core::task::Poll<$crate::stream::Result<s2n_quic_transport::stream::ops::rx::Response>>
        {
            s2n_quic_core::task::waker::contract_debug(cx, |cx| {
                let response = core::task::ready!(self
                    .rx_request()?
                    .receive(chunks)
                    // don't receive more than we're capable of storing
                    .with_high_watermark(high_watermark)
                    .poll(Some(cx))?
                    .into_poll());

                core::task::Poll::Ready(Ok(response))
            })
        }
    };
}

macro_rules! impl_receive_stream_trait {
    ($name:ident, | $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        impl futures::stream::Stream for $name {
            type Item = $crate::stream::Result<bytes::Bytes>;

            #[inline]
            fn poll_next(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Option<Self::Item>> {
                match core::task::ready!(self.poll_receive(cx)) {
                    Ok(Some(v)) => Some(Ok(v)),
                    Ok(None) => None,
                    Err(err) => Some(Err(err)),
                }
                .into()
            }
        }

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

                let response =
                    core::task::ready!(self.receive_chunks(cx, &mut chunks, high_watermark))?;

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

                let response =
                    core::task::ready!(self.receive_chunks(cx, &mut chunks, high_watermark))?;

                let chunks = &chunks[..response.chunks.consumed];
                let copied_len = s2n_quic_core::slice::vectored_copy(chunks, bufs);

                debug_assert_eq!(
                    copied_len, response.bytes.consumed,
                    "the consumed bytes should always have enough capacity in bufs"
                );

                Ok(copied_len).into()
            }
        }

        impl tokio::io::AsyncRead for $name {
            fn poll_read(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf,
            ) -> core::task::Poll<std::io::Result<()>> {
                use bytes::Bytes;

                if buf.remaining() == 0 {
                    return Ok(()).into();
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

                let high_watermark = buf.remaining();

                let response =
                    core::task::ready!(self.receive_chunks(cx, &mut chunks, high_watermark))?;

                for chunk in &chunks[..response.chunks.consumed] {
                    buf.put_slice(chunk);
                }

                Ok(()).into()
            }
        }
    };
}

impl ReceiveStream {
    #[inline]
    pub(crate) const fn new(stream: stream::ReceiveStream) -> Self {
        Self(stream)
    }

    /// Returns the stream's identifier
    ///
    /// This value is unique to a particular connection. The format follows the same as what is
    /// defined in the
    /// [QUIC Transport RFC](https://www.rfc-editor.org/rfc/rfc9000.html#name-stream-types-and-identifier).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # async fn test() -> s2n_quic::stream::Result<()> {
    /// #   let connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// while let Some(stream) = connection.accept_receive_stream().await? {
    ///     println!("New stream's id: {}", stream.id());
    /// }
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn id(&self) -> u64 {
        self.0.id().into()
    }

    impl_connection_api!(|stream| crate::connection::Handle(stream.0.connection().clone()));

    impl_receive_stream_api!(|stream, dispatch| dispatch!(stream.0));
}

impl_splittable_stream_trait!(ReceiveStream, |stream| (Some(stream), None));
impl_receive_stream_trait!(ReceiveStream, |stream, dispatch| dispatch!(stream.0));
