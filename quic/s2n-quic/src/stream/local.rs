// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{BidirectionalStream, SendStream};

/// An enum of all the possible types of QUIC streams that may be opened by a local endpoint.
///
/// The [`LocalStream`] implements the required operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub enum LocalStream {
    Bidirectional(BidirectionalStream),
    Send(SendStream),
}

impl LocalStream {
    impl_receive_stream_api!(|stream, dispatch| match stream {
        LocalStream::Bidirectional(stream) => dispatch!(stream),
        LocalStream::Send(_stream) => dispatch!(),
    });

    impl_send_stream_api!(|stream, dispatch| match stream {
        LocalStream::Bidirectional(stream) => dispatch!(stream),
        LocalStream::Send(stream) => dispatch!(stream),
    });

    #[inline]
    pub fn id(&self) -> u64 {
        match self {
            Self::Bidirectional(stream) => stream.id(),
            Self::Send(stream) => stream.id(),
        }
    }

    impl_splittable_stream_api!();

    impl_connection_api!(|stream| match stream {
        LocalStream::Bidirectional(stream) => stream.connection(),
        LocalStream::Send(stream) => stream.connection(),
    });
}

impl_receive_stream_trait!(LocalStream, |stream, dispatch| match stream {
    LocalStream::Bidirectional(stream) => dispatch!(stream),
    LocalStream::Send(_stream) => dispatch!(),
});
impl_send_stream_trait!(LocalStream, |stream, dispatch| match stream {
    LocalStream::Bidirectional(stream) => dispatch!(stream),
    LocalStream::Send(stream) => dispatch!(stream),
});
impl_splittable_stream_trait!(LocalStream, |stream| match stream {
    LocalStream::Bidirectional(stream) => super::SplittableStream::split(stream),
    LocalStream::Send(stream) => super::SplittableStream::split(stream),
});

impl From<SendStream> for LocalStream {
    #[inline]
    fn from(stream: SendStream) -> Self {
        Self::Send(stream)
    }
}

impl From<BidirectionalStream> for LocalStream {
    #[inline]
    fn from(stream: BidirectionalStream) -> Self {
        Self::Bidirectional(stream)
    }
}
