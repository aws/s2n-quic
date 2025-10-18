// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{BidirectionalStream, ReceiveStream};

/// An enum of all the possible types of QUIC streams that may be opened by a peer.
#[derive(Debug)]
pub enum PeerStream {
    Bidirectional(BidirectionalStream),
    Receive(ReceiveStream),
}

impl PeerStream {
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
    /// use s2n_quic::stream::Type;
    ///
    /// while let Some(stream) = connection.accept().await? {
    ///     println!("New stream's id: null", stream.id());
    /// }
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    pub fn id(&self) -> u64 {
        match self {
            Self::Bidirectional(stream) => stream.id(),
            Self::Receive(stream) => stream.id(),
        }
    }

    impl_connection_api!(|stream| match stream {
        PeerStream::Bidirectional(stream) => stream.connection(),
        PeerStream::Receive(stream) => stream.connection(),
    });

    impl_receive_stream_api!(|stream, dispatch| match stream {
        PeerStream::Bidirectional(stream) => dispatch!(stream),
        PeerStream::Receive(stream) => dispatch!(stream),
    });

    impl_send_stream_api!(|stream, dispatch| match stream {
        PeerStream::Bidirectional(stream) => dispatch!(stream),
        PeerStream::Receive(_stream) => dispatch!(),
    });

    impl_splittable_stream_api!();
}

impl_receive_stream_trait!(PeerStream, |stream, dispatch| match stream {
    PeerStream::Bidirectional(stream) => dispatch!(stream),
    PeerStream::Receive(stream) => dispatch!(stream),
});
impl_send_stream_trait!(PeerStream, |stream, dispatch| match stream {
    PeerStream::Bidirectional(stream) => dispatch!(stream),
    PeerStream::Receive(_stream) => dispatch!(),
});
impl_splittable_stream_trait!(PeerStream, |stream| match stream {
    PeerStream::Bidirectional(stream) => super::SplittableStream::split(stream),
    PeerStream::Receive(stream) => super::SplittableStream::split(stream),
});

impl From<ReceiveStream> for PeerStream {
    #[inline]
    fn from(stream: ReceiveStream) -> Self {
        Self::Receive(stream)
    }
}

impl From<BidirectionalStream> for PeerStream {
    #[inline]
    fn from(stream: BidirectionalStream) -> Self {
        Self::Bidirectional(stream)
    }
}