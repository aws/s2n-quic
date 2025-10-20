// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    stream::{BidirectionalStream, PeerStream, ReceiveStream},
};

macro_rules! impl_accept_api {
    () => {
        /// Accepts an incoming [`PeerStream`](`crate::stream::PeerStream`)
        ///
        /// The method will return
        /// - `Ok(Some(stream)))` if a [`PeerStream`](`crate::stream::PeerStream`) was accepted
        /// - `Ok(None)` if the connection was closed without an error
        /// - `Err(stream_error)` if no stream could be accepted due to an error
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut acceptor: s2n_quic::connection::StreamAcceptor = todo!();
        /// #
        /// while let Some(stream) = acceptor.accept().await? {
        ///     println!("Stream opened from {:?}", stream.connection().remote_addr());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn accept(
            &mut self,
        ) -> crate::connection::Result<Option<crate::stream::PeerStream>> {
            futures::future::poll_fn(|cx| self.poll_accept(cx)).await
        }

        /// Poll for accepting an incoming [`PeerStream`](`crate::stream::PeerStream`)
        ///
        /// The method will return
        /// - `Poll::Ready(Ok(Some(stream)))` if a [`PeerStream`](`crate::stream::PeerStream`) was accepted
        /// - `Poll::Ready(Ok(None))` if the connection was closed without an error
        /// - `Poll::Ready(Err(stream_error))` if no stream could be accepted due to an error
        /// - `Poll::Pending` if no new [`PeerStream`](`crate::stream::PeerStream`) was accepted by the connection yet.
        ///   In this case the caller must retry calling [`Self::poll_accept`].
        ///   For this purpose the method will save the [`core::task::Waker`]
        ///   which is provided as part of the [`core::task::Context`] parameter, and notify it
        ///   as soon as retrying the method will yield a different result.
        #[inline]
        pub fn poll_accept(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<crate::connection::Result<Option<crate::stream::PeerStream>>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::{BidirectionalStream, ReceiveStream};

            Ok(
                core::task::ready!(self.0.poll_accept(None, cx))?.map(|stream| {
                    match stream.id().stream_type() {
                        StreamType::Unidirectional => ReceiveStream::new(stream.into()).into(),
                        StreamType::Bidirectional => BidirectionalStream::new(stream).into(),
                    }
                }),
            )
            .into()
        }

        impl_accept_bidirectional_api!();
        impl_accept_receive_api!();
    };
}

macro_rules! impl_accept_bidirectional_api {
    () => {
        /// Accepts an incoming [`BidirectionalStream`](`crate::stream::BidirectionalStream`)
        ///
        /// The method will return
        /// - `Ok(Some(stream)))` if a [`BidirectionalStream`](`crate::stream::BidirectionalStream`) was accepted
        /// - `Ok(None)` if the connection was closed without an error
        /// - `Err(stream_error)` if no stream could be accepted due to an error
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut acceptor: s2n_quic::connection::StreamAcceptor = todo!();
        /// #
        /// while let Ok(Some(mut stream)) = acceptor.accept_bidirectional_stream().await {
        ///     println!("Stream opened from {:?}", stream.connection().remote_addr());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn accept_bidirectional_stream(
            &mut self,
        ) -> $crate::connection::Result<Option<$crate::stream::BidirectionalStream>> {
            futures::future::poll_fn(|cx| self.poll_accept_bidirectional_stream(cx)).await
        }

        /// Poll for accepting an incoming [`BidirectionalStream`](`crate::stream::BidirectionalStream`)
        ///
        /// The method will return
        /// - `Poll::Ready(Ok(Some(stream)))` if a [`BidirectionalStream`](`crate::stream::BidirectionalStream`) was accepted
        /// - `Poll::Ready(Ok(None))` if the connection was closed without an error
        /// - `Poll::Ready(Err(stream_error))` if no stream could be accepted due to an error
        /// - `Poll::Pending` if no new [`BidirectionalStream`](`crate::stream::BidirectionalStream`) was accepted by the connection yet.
        ///   In this case the caller must retry calling [`Self::poll_accept_bidirectional_stream`].
        ///   For this purpose the method will save the [`core::task::Waker`]
        ///   which is provided as part of the [`core::task::Context`] parameter, and notify it
        ///   as soon as retrying the method will yield a different result.
        #[inline]
        pub fn poll_accept_bidirectional_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::BidirectionalStream>>> {
            Ok(
                core::task::ready!(self
                    .0
                    .poll_accept(Some(s2n_quic_core::stream::StreamType::Bidirectional), cx)
                )?.map($crate::stream::BidirectionalStream::new)
            ).into()
        }
    };
}

macro_rules! impl_accept_receive_api {
    () => {
        /// Accepts an incoming [`ReceiveStream`](`crate::stream::ReceiveStream`)
        ///
        /// The method will return
        /// - `Ok(Some(stream)))` if a [`ReceiveStream`](`crate::stream::ReceiveStream`) was accepted
        /// - `Ok(None)` if the connection was closed without an error
        /// - `Err(stream_error)` if no stream could be accepted due to an error
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::stream::Result<()> {
        /// #   let mut acceptor: s2n_quic::connection::StreamAcceptor = todo!();
        /// #
        /// while let Ok(Some(mut stream)) = acceptor.accept_receive_stream().await {
        ///     println!("Stream opened from {:?}", stream.connection().remote_addr());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn accept_receive_stream(
            &mut self,
        ) -> $crate::connection::Result<Option<$crate::stream::ReceiveStream>> {
            futures::future::poll_fn(|cx| self.poll_accept_receive_stream(cx)).await
        }

        /// Poll for accepting an incoming [`ReceiveStream`](`crate::stream::ReceiveStream`)
        ///
        /// The method will return
        /// - `Poll::Ready(Ok(Some(stream)))` if a [`ReceiveStream`](`crate::stream::ReceiveStream`) was accepted
        /// - `Poll::Ready(Ok(None))` if the connection was closed without an error
        /// - `Poll::Ready(Err(stream_error))` if no stream could be accepted due to an error
        /// - `Poll::Pending` if no new [`ReceiveStream`](`crate::stream::ReceiveStream`) was accepted by the connection yet.
        ///   In this case the caller must retry calling [`Self::poll_accept_receive_stream`].
        ///   For this purpose the method will save the [`core::task::Waker`]
        ///   which is provided as part of the [`core::task::Context`] parameter, and notify it
        ///   as soon as retrying the method will yield a different result.
        #[inline]
        pub fn poll_accept_receive_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::ReceiveStream>>> {
            Ok(core::task::ready!(self
                .0
                .poll_accept(Some(s2n_quic_core::stream::StreamType::Unidirectional), cx))?
            .map(|stream| $crate::stream::ReceiveStream::new(stream.into())))
            .into()
        }
    };
}

#[derive(Debug)]
pub struct StreamAcceptor(pub(crate) s2n_quic_transport::connection::Connection);

impl StreamAcceptor {
    impl_accept_api!();

    /// Splits the [`StreamAcceptor`] into [`BidirectionalStreamAcceptor`] and [`ReceiveStreamAcceptor`] halves
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use bytes::Bytes;
    /// # async fn test() -> s2n_quic::stream::Result<()> {
    /// #   let connection: s2n_quic::connection::Connection = todo!();
    /// #
    /// let (handle, acceptor) = connection.split();
    /// let (mut bidi, mut recv) = acceptor.split();
    ///
    /// tokio::spawn(async move {
    ///     while let Ok(Some(mut stream)) = bidi.accept_bidirectional_stream().await {
    ///       println!("Bidirectional stream opened from {:?}", stream.connection().remote_addr());
    ///     }
    /// });
    ///
    /// tokio::spawn(async move {
    ///     while let Ok(Some(mut stream)) = recv.accept_receive_stream().await {
    ///       println!("Receive stream opened from {:?}", stream.connection().remote_addr());
    ///     }
    /// });
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn split(self) -> (BidirectionalStreamAcceptor, ReceiveStreamAcceptor) {
        let bidi = BidirectionalStreamAcceptor(self.0.clone());
        let recv = ReceiveStreamAcceptor(self.0);
        (bidi, recv)
    }
}

impl futures::stream::Stream for StreamAcceptor {
    type Item = connection::Result<PeerStream>;

    #[inline]
    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        match core::task::ready!(self.poll_accept(cx)) {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
        .into()
    }
}

#[derive(Debug)]
pub struct BidirectionalStreamAcceptor(s2n_quic_transport::connection::Connection);

impl BidirectionalStreamAcceptor {
    impl_accept_bidirectional_api!();
}

impl futures::stream::Stream for BidirectionalStreamAcceptor {
    type Item = connection::Result<BidirectionalStream>;

    #[inline]
    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        match core::task::ready!(self.poll_accept_bidirectional_stream(cx)) {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
        .into()
    }
}

#[derive(Debug)]
pub struct ReceiveStreamAcceptor(s2n_quic_transport::connection::Connection);

impl ReceiveStreamAcceptor {
    impl_accept_receive_api!();
}

impl futures::stream::Stream for ReceiveStreamAcceptor {
    type Item = connection::Result<ReceiveStream>;

    #[inline]
    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        match core::task::ready!(self.poll_accept_receive_stream(cx)) {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
        .into()
    }
}
