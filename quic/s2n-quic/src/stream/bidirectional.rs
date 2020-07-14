/// An QUIC stream that may both send and receive data.
///
/// The [`BidirectionalStream`] implements the required send and receive operations described in the
/// [QUIC Transport RFC](https://tools.ietf.org/html/draft-ietf-quic-transport-28#section-2)
#[derive(Debug)]
pub struct BidirectionalStream {
    // TODO
}

impl BidirectionalStream {
    impl_receive_stream_api!(|stream, call| call!(stream));

    impl_send_stream_api!(|stream, call| call!(stream));

    impl_splittable_stream_api!(|_stream| {
        todo!();
    });

    impl_connection_api!(|_stream| todo!());

    impl_metric_api!();
}

impl_receive_stream_trait!(BidirectionalStream, |stream, call| call!(stream));
impl_send_stream_trait!(BidirectionalStream, |stream, call| call!(stream));
impl_splittable_stream_trait!(BidirectionalStream, |_stream| {
    todo!();
});
