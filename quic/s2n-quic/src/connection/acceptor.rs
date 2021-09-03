// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    stream::{BidirectionalStream, PeerStream, ReceiveStream},
};

macro_rules! impl_accept_api {
    () => {
        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub async fn accept(
            &mut self,
        ) -> crate::connection::Result<Option<crate::stream::PeerStream>> {
            futures::future::poll_fn(|cx| self.poll_accept(cx)).await
        }

        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
        pub fn poll_accept(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<crate::connection::Result<Option<crate::stream::PeerStream>>> {
            use s2n_quic_core::stream::StreamType;
            use $crate::stream::{BidirectionalStream, ReceiveStream};

            Ok(
                futures::ready!(self.0.poll_accept(None, cx))?.map(|stream| {
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
        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
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
        #[inline]
        pub fn poll_accept_bidirectional_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::BidirectionalStream>>> {
            Ok(
                futures::ready!(self
                    .0
                    .poll_accept(Some(s2n_quic_core::stream::StreamType::Bidirectional), cx)
                )?.map($crate::stream::BidirectionalStream::new)
            ).into()
        }
    };
}

macro_rules! impl_accept_receive_api {
    () => {
        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        #[inline]
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
        #[inline]
        pub fn poll_accept_receive_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<Option<$crate::stream::ReceiveStream>>> {
            Ok(futures::ready!(self
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

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
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
        match futures::ready!(self.poll_accept(cx)) {
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
        match futures::ready!(self.poll_accept_bidirectional_stream(cx)) {
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
        match futures::ready!(self.poll_accept_receive_stream(cx)) {
            Ok(Some(stream)) => Some(Ok(stream)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
        .into()
    }
}
