// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_transport::stream;

/// A QUIC stream that is only allowed to send data.
///
/// The [`SendStream`] implements the required send operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct SendStream(stream::SendStream);

macro_rules! impl_send_stream_api {
    (| $stream:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Pushes data onto the stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub async fn send(&mut self, mut data: bytes::Bytes) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_send(&mut data, cx)).await
        }

        /// Polls sending a slice of chunked data on the stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn poll_send(
            &mut self,
            chunk: &mut bytes::Bytes,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.poll_send(chunk, cx)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Pushes a slice of chunked data onto the stream.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub async fn send_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
        ) -> $crate::stream::Result<()> {
            let mut sent_chunks = 0;

            ::futures::future::poll_fn(|cx| {
                sent_chunks +=
                    ::futures::ready!(self.poll_send_vectored(&mut chunks[sent_chunks..], cx))?;
                if sent_chunks == chunks.len() {
                    return Ok(()).into();
                }
                core::task::Poll::Pending
            })
            .await
        }

        /// Polls sending a slice of chunked data on the stream
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn poll_send_vectored(
            &mut self,
            chunks: &mut [bytes::Bytes],
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<usize>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.poll_send_vectored(chunks, cx)
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
        #[inline]
        pub fn poll_send_ready(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<usize>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.poll_send_ready(cx)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Sends data on the stream.
        ///
        /// `Self::poll_send_ready` _must_ be called before calling this method.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn send_data(&mut self, data: bytes::Bytes) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable)
                };
                ($variant: expr) => {
                    $variant.send_data(data)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Flushes the stream and waits for the peer to receive all outstanding data
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub async fn flush(&mut self) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_flush(cx)).await
        }

        /// Polls flushing the stream and waits for the peer to receive all outstanding data
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn poll_flush(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.poll_flush(cx)
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Finishes and closes the stream.
        ///
        /// This method returns immediately without notifying the caller that all of the outstanding
        /// data has been received by the peer. An application can use `close` to accomplish this.
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn finish(&mut self) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.finish()
                };
            }

            let $stream = self;
            $dispatch_body
        }

        /// Finishes the stream and waits for the peer to receive all outstanding data
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub async fn close(&mut self) -> $crate::stream::Result<()> {
            ::futures::future::poll_fn(|cx| self.poll_close(cx)).await
        }

        /// Polls finishing the stream and waits for the peer to receive all outstanding data
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn poll_close(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::stream::Result<()>> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable).into()
                };
                ($variant: expr) => {
                    $variant.poll_close(cx)
                };
            }

            let $stream = self;
            $dispatch_body
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
        #[inline]
        pub fn reset(
            &mut self,
            error_code: $crate::application::Error,
        ) -> $crate::stream::Result<()> {
            macro_rules! $dispatch {
                () => {
                    Err($crate::stream::Error::NonWritable)
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
                futures::ready!(self.poll_send_ready(cx))?;
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

        #[cfg(feature = "std")]
        impl futures::io::AsyncWrite for $name {
            fn poll_write(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
                buf: &[u8],
            ) -> core::task::Poll<std::io::Result<usize>> {
                if buf.is_empty() {
                    return Ok(0).into();
                }

                let len = futures::ready!(self.poll_send_ready(cx))?.min(buf.len());
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

                let len = futures::ready!(self.poll_send_ready(cx))?;
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
                futures::ready!($name::poll_flush(&mut self, cx))?;
                Ok(()).into()
            }

            #[inline]
            fn poll_close(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context<'_>,
            ) -> core::task::Poll<std::io::Result<()>> {
                futures::ready!($name::poll_close(&mut self, cx))?;
                Ok(()).into()
            }
        }

        #[cfg(all(feature = "std", feature = "tokio"))]
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

                let len = futures::ready!(self.poll_send_ready(cx))?;
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
                futures::ready!(self.poll_close(cx))?;
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

    impl_send_stream_api!(|stream, dispatch| dispatch!(stream.0));

    #[inline]
    pub fn id(&self) -> u64 {
        self.0.id().into()
    }

    impl_connection_api!(|stream| crate::connection::Handle(stream.0.connection().clone()));
}

impl_splittable_stream_trait!(SendStream, |stream| (None, Some(stream)));
impl_send_stream_trait!(SendStream, |stream, dispatch| dispatch!(stream.0));
