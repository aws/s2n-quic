// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_transport::stream::Stream;

/// A QUIC stream that may send and receive data.
#[derive(Debug)]
pub struct BidirectionalStream(Stream);

impl BidirectionalStream {
    pub(crate) const fn new(stream: Stream) -> Self {
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
    /// let stream = connection.open_bidirectional_stream().await?;
    /// println!("New stream's id: {}", stream.id());
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    pub fn id(&self) -> u64 {
        self.0.id().into()
    }

    impl_connection_api!(|stream| crate::connection::Handle(stream.0.connection().clone()));

    impl_receive_stream_api!(|stream, call| call!(stream.0));

    impl_send_stream_api!(|stream, call| call!(stream.0));

    /// Splits the stream into [`ReceiveStream`](crate::stream::ReceiveStream) and
    /// [`SendStream`](crate::stream::SendStream) halves.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use bytes::Bytes;
    /// # async fn test() -> s2n_quic::stream::Result<()> {
    /// #   let connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// let stream = connection.open_bidirectional_stream().await?;
    /// let (recv, mut send) = stream.split();
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
    pub fn split(self) -> (crate::stream::ReceiveStream, crate::stream::SendStream) {
        let (recv, send) = self.0.split();
        (
            crate::stream::ReceiveStream::new(recv),
            crate::stream::SendStream::new(send),
        )
    }
}

impl_receive_stream_trait!(BidirectionalStream, |stream, call| call!(stream.0));
impl_send_stream_trait!(BidirectionalStream, |stream, call| call!(stream.0));
impl_splittable_stream_trait!(BidirectionalStream, |stream| {
    let (recv, send) = Self::split(stream);
    (Some(recv), Some(send))
});
