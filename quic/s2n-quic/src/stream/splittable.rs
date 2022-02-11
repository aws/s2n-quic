// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{ReceiveStream, SendStream};

/// A trait that enables a stream to split into a [`ReceiveStream`] and [`SendStream`].
///
/// Note that if a stream is only allowed to send, then the receiving side will be [`None`].
/// The same is true for streams that are only allowed to receive: the sending side will be [`None`].
pub trait SplittableStream {
    /// Splits the stream into [`ReceiveStream`] and [`SendStream`] halves
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use bytes::Bytes;
    /// # async fn test() -> s2n_quic::stream::Result<()> {
    /// #   let connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// let stream = connection.open_bidirectional_stream().await?;
    /// let (recv, send) = s2n_quic::stream::SplittableStream::split(stream);
    /// let mut recv = recv.expect("bidirectional streams have receiving sides");
    /// let mut send = send.expect("bidirectional streams have sending sides");
    ///
    /// tokio::spawn(async move {
    ///     let _ = send.send(Bytes::from_static(&[1, 2, 3])).await;
    /// });
    ///
    /// while let Some(chunk) = recv.receive().await? {
    ///     println!("received: {:?}", chunk);
    /// }
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    fn split(self) -> (Option<ReceiveStream>, Option<SendStream>);
}

macro_rules! impl_splittable_stream_api {
    () => {
        /// Splits the stream into [`ReceiveStream`](crate::stream::ReceiveStream) and
        /// [`SendStream`](crate::stream::SendStream) halves
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// # let stream: s2n_quic::stream::Stream = todo!();
        /// #
        /// let (recv, send) = stream.split();
        ///
        /// if let Some(recv) = recv {
        ///    // the stream has a receive half
        /// }
        ///
        /// if let Some(send) = send {
        ///    // the stream has a send half
        /// }
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub fn split(
            self,
        ) -> (
            Option<$crate::stream::ReceiveStream>,
            Option<$crate::stream::SendStream>,
        ) {
            $crate::stream::SplittableStream::split(self)
        }
    };
}

macro_rules! impl_splittable_stream_trait {
    ($name:ident, | $self:ident | $convert:expr) => {
        impl $crate::stream::SplittableStream for $name {
            #[inline]
            fn split(
                self,
            ) -> (
                Option<$crate::stream::ReceiveStream>,
                Option<$crate::stream::SendStream>,
            ) {
                let $self = self;
                $convert
            }
        }
    };
}
