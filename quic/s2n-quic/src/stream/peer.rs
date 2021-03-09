// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{BidirectionalStream, ReceiveStream};

/// An enum of all the possible types of QUIC streams that may be opened by a peer.
///
/// The [`PeerStream`] implements the required operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub enum PeerStream {
    Bidirectional(BidirectionalStream),
    Receive(ReceiveStream),
}

impl PeerStream {
    impl_receive_stream_api!(|stream, dispatch| match stream {
        PeerStream::Bidirectional(stream) => dispatch!(stream),
        PeerStream::Receive(stream) => dispatch!(stream),
    });

    impl_send_stream_api!(|stream, dispatch| match stream {
        PeerStream::Bidirectional(stream) => dispatch!(stream),
        PeerStream::Receive(_stream) => dispatch!(),
    });

    impl_splittable_stream_api!();

    impl_connection_api!(|stream| match stream {
        PeerStream::Bidirectional(stream) => stream.connection(),
        PeerStream::Receive(stream) => stream.connection(),
    });
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
    fn from(stream: ReceiveStream) -> Self {
        Self::Receive(stream)
    }
}

impl From<BidirectionalStream> for PeerStream {
    fn from(stream: BidirectionalStream) -> Self {
        Self::Bidirectional(stream)
    }
}
