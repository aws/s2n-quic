use crate::{
    connection,
    stream::{BidirectionalStream, PeerStream, ReceiveStream},
};

macro_rules! impl_acceptor_api {
    (| $acceptor:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn accept(&mut self) -> $crate::connection::Result<Option<$crate::stream::PeerStream>> {
            futures::future::poll_fn(|cx| self.poll_accept(cx)).await
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_accept(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::PeerStream>>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::{BidirectionalStream, ReceiveStream};

            Ok(futures::ready!(self.0.poll_accept(None, cx))?.map(|stream| match stream {
                (stream, StreamType::Unidirectional) => ReceiveStream::new(stream).into(),
                (stream, StreamType::Bidirectional) => BidirectionalStream::new(stream).into(),
            }))
            .into()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn accept_bidirectional_stream(
            &mut self,
        ) -> $crate::connection::Result<Option<$crate::stream::BidirectionalStream>> {
            futures::future::poll_fn(|cx| self.poll_accept_bidirectional_stream(cx)).await
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_accept_bidirectional_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::BidirectionalStream>>> {
            Ok(futures::ready!(self
                .0
                .poll_accept(Some(s2n_quic_core::stream::StreamType::Bidirectional), cx))?.map(|(stream, _)|
                    $crate::stream::BidirectionalStream::new(stream)
                )).into()
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn accept_receive_stream(
            &mut self,
        ) -> $crate::connection::Result<Option<$crate::stream::ReceiveStream>> {
            futures::future::poll_fn(|cx| self.poll_accept_receive_stream(cx)).await
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn poll_accept_receive_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::ReceiveStream>>> {
            Ok(futures::ready!(self
                .0
                .poll_accept(Some(s2n_quic_core::stream::StreamType::Unidirectional), cx))?.map(|(stream, _)|
                    $crate::stream::ReceiveStream::new(stream)
                )).into()
        }
    };
}

#[derive(Debug)]
pub struct StreamAcceptor(s2n_quic_transport::connection::Connection);

impl StreamAcceptor {
    impl_acceptor_api!(|acceptor, call| call!(acceptor));

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn split(self) -> (BidirectionalStreamAcceptor, ReceiveStreamAcceptor) {
        todo!()
    }
}

impl futures::stream::Stream for StreamAcceptor {
    type Item = connection::Result<PeerStream>;

    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        match futures::ready!(self.poll_accept(cx)) {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
        .into()
    }
}

#[derive(Debug)]
pub struct BidirectionalStreamAcceptor {
    // TODO
}

impl BidirectionalStreamAcceptor {
    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub async fn accept(&mut self) -> connection::Result<BidirectionalStream> {
        todo!()
    }

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn poll_accept(
        &mut self,
        cx: &mut core::task::Context,
    ) -> core::task::Poll<connection::Result<BidirectionalStream>> {
        let _ = cx;
        todo!()
    }
}

impl futures::stream::Stream for BidirectionalStreamAcceptor {
    type Item = connection::Result<BidirectionalStream>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let _ = cx;
        todo!()
    }
}

#[derive(Debug)]
pub struct ReceiveStreamAcceptor {
    // TODO
}

impl ReceiveStreamAcceptor {
    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub async fn accept(&mut self) -> connection::Result<ReceiveStream> {
        todo!()
    }

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn poll_accept(
        &mut self,
        cx: &mut core::task::Context,
    ) -> core::task::Poll<connection::Result<ReceiveStream>> {
        let _ = cx;
        todo!()
    }
}

impl futures::stream::Stream for ReceiveStreamAcceptor {
    type Item = connection::Result<ReceiveStream>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let _ = cx;
        todo!()
    }
}
