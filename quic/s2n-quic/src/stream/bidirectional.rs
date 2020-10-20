use s2n_quic_transport::stream::Stream;

/// An QUIC stream that may both send and receive data.
///
/// The [`BidirectionalStream`] implements the required send and receive operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct BidirectionalStream(Stream);

impl BidirectionalStream {
    pub(crate) const fn new(stream: Stream) -> Self {
        Self(stream)
    }

    impl_receive_stream_api!(|stream, call| call!(stream.0));

    impl_send_stream_api!(|stream, call| call!(stream.0));

    impl_splittable_stream_api!(|_stream| {
        todo!();
    });

    impl_connection_api!(|_stream| todo!());

    impl_metric_api!();
}

impl_receive_stream_trait!(BidirectionalStream, |stream, call| call!(stream.0));
impl_send_stream_trait!(BidirectionalStream, |stream, call| call!(stream.0));
impl_splittable_stream_trait!(BidirectionalStream, |_stream| {
    todo!();
});
