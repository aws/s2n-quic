// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_transport::stream;

/// A QUIC stream that is only allowed to send data.
#[derive(Debug)]
pub struct SendStream(stream::SendStream);

macro_rules! impl_send_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Enqueues a chunk of data for sending it towards the peer.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Ok(())` if the data was enqueued for sending.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let stream: s2n_quic::stream::SendStream = todo!();
        /// #
        /// let data = bytes::Bytes::from_static(&[1, 2, 3, 4]);
        /// stream.send(data).await?;
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn send(&mut self, mut data: bytes::Bytes) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_send(&mut data, cx)).await
        }

        /// Enqueues a chunk of data for sending it towards the peer.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Poll::Pending` if the stream's send buffer capacity is currently exhausted. In this case,
        ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(()))` if the data was enqueued for sending. The provided `chunk` will
        ///   be replaced with an empty [`Bytes`](bytes::Bytes).
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_send(
            &mut self,
            chunk: &mut bytes::Bytes,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                        $variant.poll_send(chunk, cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Enqueues a slice of chunks of data for sending it towards the peer.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Ok(())` if all of the chunks of data were enqueued for sending. Each of the
        ///   consumed [`Bytes`](bytes::Bytes) will be replaced with an empty [`Bytes`](bytes::Bytes).
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let stream: s2n_quic::stream::SendStream = todo!();
        /// #
        /// let mut data1 = bytes::Bytes::from_static(&[1, 2, 3]);
        /// let mut data2 = bytes::Bytes::from_static(&[4, 5, 6]);
        /// let mut data3 = bytes::Bytes::from_static(&[7, 8, 9]);
        /// let chunks = [data1, data2, data3];
        /// stream.send_vectored(&mut chunks).await?;
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn send_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
        ) -> $crate::stream::Result<()> {
            let mut sent_chunks = 0;

            ::futures::future::poll_fn(|cx| {
                sent_chunks +=
                    ::core::task::ready!(self.poll_send_vectored(&mut chunks[sent_chunks..], cx))?;
                if sent_chunks == chunks.len() {
                    return Ok(()).into();
                }
                core::task::Poll::Pending
            })
            .await
        }

        /// Polls enqueueing a slice of chunks of data for sending it towards the peer.
        ///
        /// # Return value
        ///
        /// The function returns:
        ///
        /// - `Poll::Pending` if the stream's send buffer capacity is currently exhausted. In this case,
        ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(count))` if one or more chunks of data were enqueued for sending. Any of the
        ///   consumed [`Bytes`](bytes::Bytes) will be replaced with an empty [`Bytes`](bytes::Bytes).
        ///   If `count` does not equal the total number of chunks, the stream will store the
        ///   [Waker](core::task::Waker) and notify the task once more capacity is available.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_send_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<usize>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                        $variant.poll_send_vectored(chunks, cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Polls send readiness for the given stream.
        ///
        /// This method _must_ be called before calling [`send_data`](Self::send_data).
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Poll::Pending` if the stream's send buffer capacity is currently exhausted. In this case,
        ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(available_bytes))` if the stream is ready to send data, where
        ///   `available_bytes` is how many bytes the stream can currently accept.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_send_ready(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<usize>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                        $variant.poll_send_ready(cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Sends data on the stream without blocking the task.
        ///
        /// [`poll_send_ready`](Self::poll_send_ready) _must_ be called before calling this method.
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Ok(())` if the data was enqueued for sending.
        /// - `Err(SendingBlocked)` if the stream did not have enough capacity to enqueue the
        ///   `chunk`.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn send_data(&mut self, chunk: bytes::Bytes) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable())
                };
                ($variant: expr) => {
                    $variant.send_data(chunk)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Flushes the stream and waits for the peer to receive all outstanding data.
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Ok(())` if the send buffer was completely flushed and acknowledged by
        ///   the peer.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let stream: s2n_quic::stream::SendStream = todo!();
        /// #
        /// let data = bytes::Bytes::from_static(&[1, 2, 3, 4]);
        /// stream.send(data).await?;
        /// stream.flush().await?;
        /// // at this point, the peer has received all of the `data`
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn flush(&mut self) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_flush(cx)).await
        }

        /// Polls flushing the stream and waits for the peer to receive all outstanding data.
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Poll::Pending` if the stream's send buffer is still being sent. In this case,
        ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(()))` if the send buffer was completely flushed and acknowledged by
        ///   the peer.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_flush(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                        $variant.poll_flush(cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Marks the stream as finished.
        ///
        /// This method returns immediately without notifying the caller that all of the outstanding
        /// data has been received by the peer. An application wanting to both [`finish`](Self::finish)
        /// and [`flush`](Self::flush) the outstanding data can use [`close`](Self::close) to accomplish
        /// this.
        ///
        /// __NOTE__: This method will be called when the [`stream`](Self) is dropped.
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Ok(())` if the stream was finished successfully.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn finish(&mut self) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    $variant.finish()
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Marks the stream as finished and waits for all outstanding data to be acknowledged.
        ///
        /// This method is equivalent to calling [`finish`](Self::finish) and [`flush`](Self::flush).
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Ok(())` if the send buffer was completely flushed and acknowledged by
        ///   the peer.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error).
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let stream: s2n_quic::stream::SendStream = todo!();
        /// #
        /// let data = bytes::Bytes::from_static(&[1, 2, 3, 4]);
        /// stream.send(data).await?;
        /// stream.close().await?;
        /// // at this point, the peer has received all of the `data` and has acknowledged the
        /// // stream being finished.
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn close(&mut self) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_close(cx)).await
        }

        /// Marks the stream as finished and polls for all outstanding data to be acknowledged.
        ///
        /// This method is equivalent to calling [`finish`](Self::finish) and [`flush`](Self::flush).
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Poll::Pending` if the stream's send buffer is still being sent. In this case,
        ///   the caller should retry sending after the [`Waker`](core::task::Waker) on the provided
        ///   [`Context`](core::task::Context) is notified.
        /// - `Poll::Ready(Ok(()))` if the send buffer was completely flushed and acknowledged by
        ///   the peer.
        /// - `Poll::Ready(Err(e))` if the stream encountered a [`stream::Error`](crate::stream::Error).
        #[inline]
        pub fn poll_close(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable()).into()
                };
                ($variant: expr) => {
                    s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                        $variant.poll_close(cx)
                    })
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Closes the stream with an [error code](crate::application::Error).
        ///
        /// After calling this, the stream is closed and will not accept any additional data to be
        /// sent to the peer. The peer will also be notified of the [error
        /// code](crate::application::Error).
        ///
        /// # Return value
        ///
        /// The function returns:
        /// - `Ok(())` if the stream was reset successfully.
        /// - `Err(e)` if the stream encountered a [`stream::Error`](crate::stream::Error). The
        ///   stream may have been reset previously, or the connection itself was closed.
        #[inline]
        pub fn reset(
            &mut self,
            error_code: $crate::application::Error,
        ) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::non_writable())
                };
                ($variant: expr) => {
                    $variant.reset(error_code)
                };
            }

            let $stream = self;
            $dispatch_body
        }
    };
}

macro_rules! impl_send_stream_trait {
    ($name:ident, | $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        impl futures::sink::Sink<bytes::Bytes> for $name {
            type Error = $crate::stream::Error;

            #[inline]
            fn poll_ready(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                core::task::ready!(self.poll_send_ready(cx))?;
                Ok(()).into()
            }

            #[inline]
            fn start_send(
                mut self: core::pin::Pin<&mut Self>,
                data: bytes::Bytes,
            ) -> $crate::stream::Result<()> {
                self.send_data(data)
            }

            #[inline]
            fn poll_flush(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                Self::poll_flush(&mut self, cx)
            }

            #[inline]
            fn poll_close(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<$crate::stream::Result<()>> {
                Self::poll_close(&mut self, cx)
            }
        }

        impl futures::io::AsyncWrite for $name {
            fn poll_write(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &[u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                if buf.is_empty() {
                    return Ok(0).into();
                }

                let len = core::task::ready!(self.poll_send_ready(cx))?.min(buf.len());
                let data = bytes::Bytes::copy_from_slice(&buf[..len]);
                self.send_data(data)?;
                Ok(len).into()
            }

            fn poll_write_vectored(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                bufs: &[futures::io::IoSlice],
            ) -> core::task::Poll<std::io::Result<usize>> {
                if bufs.is_empty() {
                    return Ok(0).into();
                }

                let len = core::task::ready!(self.poll_send_ready(cx))?;
                let capacity = bufs.iter().map(|buf| buf.len()).sum();
                let len = len.min(capacity);

                let mut data = bytes::BytesMut::with_capacity(len);
                for buf in bufs {
                    // only copy what the window will allow
                    let to_copy = buf.len().min(len - data.len());
                    data.extend_from_slice(&buf[..to_copy]);

                    // we're done filling the buffer
                    if data.len() == len {
                        break;
                    }
                }

                self.send_data(data.freeze())?;

                Ok(len).into()
            }

            #[inline]
            fn poll_flush(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                core::task::ready!($name::poll_flush(&mut self, cx))?;
                Ok(()).into()
            }

            #[inline]
            fn poll_close(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                core::task::ready!($name::poll_close(&mut self, cx))?;
                Ok(()).into()
            }
        }

        impl tokio::io::AsyncWrite for $name {
            #[inline]
            fn poll_write(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &[u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                futures::io::AsyncWrite::poll_write(self, cx, buf)
            }

            fn poll_write_vectored(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                bufs: &[std::io::IoSlice],
            ) -> core::task::Poll<std::io::Result<usize>> {
                if bufs.is_empty() {
                    return Ok(0).into();
                }

                let len = core::task::ready!(self.poll_send_ready(cx))?;
                let capacity = bufs.iter().map(|buf| buf.len()).sum();
                let len = len.min(capacity);

                let mut data = bytes::BytesMut::with_capacity(len);
                for buf in bufs {
                    // only copy what the window will allow
                    let to_copy = buf.len().min(len - data.len());
                    data.extend_from_slice(&buf[..to_copy]);

                    // we're done filling the buffer
                    if data.len() == len {
                        break;
                    }
                }

                self.send_data(data.freeze())?;

                Ok(len).into()
            }

            #[inline]
            fn is_write_vectored(&self) -> bool {
                true
            }

            #[inline]
            fn poll_flush(
                self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                futures::io::AsyncWrite::poll_flush(self, cx)
            }

            #[inline]
            fn poll_shutdown(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                core::task::ready!(self.poll_close(cx))?;
                Ok(()).into()
            }
        }
    };
}

impl SendStream {
    #[inline]
    pub(crate) const fn new(stream: stream::SendStream) -> Self {
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
    /// let stream = connection.open_send_stream().await?;
    /// println!("New stream's id: {}", stream.id());
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn id(&self) -> u64 {
        self.0.id().into()
    }

    impl_connection_api!(|stream| crate::connection::Handle(stream.0.connection().clone()));

    impl_send_stream_api!(|stream, dispatch| dispatch!(stream.0));
}

impl_splittable_stream_trait!(SendStream, |stream| (None, Some(stream)));
impl_send_stream_trait!(SendStream, |stream, dispatch| dispatch!(stream.0));
