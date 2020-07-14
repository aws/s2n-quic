#[macro_use]
mod connection;
#[macro_use]
pub mod metric;
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
pub use metric::Metric;
pub use peer::*;
pub use receive::*;
pub use send::*;
pub use splittable::*;

pub type Result<T> = core::result::Result<T, Error>;

/// An enum of all the possible types of QUIC streams.
///
/// The [`Stream`] implements the required operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub enum Stream {
    Bidirectional(BidirectionalStream),
    Receive(ReceiveStream),
    Send(SendStream),
}

impl Stream {
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

    impl_splittable_stream_api!(|stream| match stream {
        Stream::Bidirectional(stream) => stream.split(),
        Stream::Receive(stream) => stream.split(),
        Stream::Send(stream) => stream.split(),
    });

    impl_connection_api!(|stream| match stream {
        Stream::Bidirectional(stream) => stream.connection(),
        Stream::Receive(stream) => stream.connection(),
        Stream::Send(stream) => stream.connection(),
    });

    impl_metric_api!();
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
    Stream::Bidirectional(stream) => stream.split(),
    Stream::Receive(stream) => stream.split(),
    Stream::Send(stream) => stream.split(),
});
