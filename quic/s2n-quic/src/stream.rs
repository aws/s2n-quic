// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
mod connection;
#[macro_use]
mod splittable;
#[macro_use]
mod receive;
#[macro_use]
mod send;
#[macro_use]
mod bidirectional;

mod local;
mod peer;

pub use s2n_quic_core::stream::{StreamError as Error, StreamType as Type};

pub use bidirectional::*;
pub use local::*;
pub use peer::*;
pub use receive::*;
pub use send::*;
pub use splittable::*;

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// An enum of all the possible types of QUIC streams.
#[derive(Debug)]
pub enum Stream {
    Bidirectional(BidirectionalStream),
    Receive(ReceiveStream),
    Send(SendStream),
}

impl Stream {
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
    /// use s2n_quic::stream::Stream;
    ///
    /// let stream = connection.open_bidirectional_stream().await?;
    /// let stream: Stream = stream.into();
    /// println!("New stream's id: {}", stream.id());
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn id(&self) -> u64 {
        match self {
            Self::Bidirectional(stream) => stream.id(),
            Self::Receive(stream) => stream.id(),
            Self::Send(stream) => stream.id(),
        }
    }

    impl_connection_api!(|stream| match stream {
        Stream::Bidirectional(stream) => stream.connection(),
        Stream::Receive(stream) => stream.connection(),
        Stream::Send(stream) => stream.connection(),
    });

    impl_receive_stream_api!(|stream, dispatch| match stream {
        Stream::Bidirectional(stream) => dispatch!(stream),
        Stream::Receive(stream) => dispatch!(stream),
        Stream::Send(_stream) => dispatch!(),
    });

    impl_send_stream_api!(|stream, dispatch| match stream {
        Stream::Bidirectional(stream) => dispatch!(stream),
        Stream::Receive(_stream) => dispatch!(),
        Stream::Send(stream) => dispatch!(stream),
    });

    impl_splittable_stream_api!();
}

impl_receive_stream_trait!(Stream, |stream, dispatch| match stream {
    Stream::Bidirectional(stream) => dispatch!(stream),
    Stream::Receive(stream) => dispatch!(stream),
    Stream::Send(_stream) => dispatch!(),
});
impl_send_stream_trait!(Stream, |stream, dispatch| match stream {
    Stream::Bidirectional(stream) => dispatch!(stream),
    Stream::Receive(_stream) => dispatch!(),
    Stream::Send(stream) => dispatch!(stream),
});
impl_splittable_stream_trait!(Stream, |stream| match stream {
    Stream::Bidirectional(stream) => SplittableStream::split(stream),
    Stream::Receive(stream) => SplittableStream::split(stream),
    Stream::Send(stream) => SplittableStream::split(stream),
});

impl From<ReceiveStream> for Stream {
    #[inline]
    fn from(stream: ReceiveStream) -> Self {
        Self::Receive(stream)
    }
}

impl From<SendStream> for Stream {
    #[inline]
    fn from(stream: SendStream) -> Self {
        Self::Send(stream)
    }
}

impl From<BidirectionalStream> for Stream {
    #[inline]
    fn from(stream: BidirectionalStream) -> Self {
        Self::Bidirectional(stream)
    }
}
