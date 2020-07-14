use crate::stream::{self, BidirectionalStream, PeerStream, ReceiveStream};

macro_rules! impl_acceptor_api {
    (| $acceptor:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// TODO
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub async fn accept(&mut self) -> $crate::stream::Result<$crate::stream::PeerStream> {
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
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::PeerStream>> {
            let _ = cx;
            todo!()
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
        ) -> $crate::stream::Result<$crate::stream::BidirectionalStream> {
            todo!()
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
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::BidirectionalStream>> {
            let _ = cx;
            todo!()
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
        ) -> $crate::stream::Result<$crate::stream::ReceiveStream> {
            todo!()
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
        ) -> core::task::Poll<$crate::stream::Result<$crate::stream::ReceiveStream>> {
            let _ = cx;
            todo!()
        }
    };
}

#[derive(Debug)]
pub struct StreamAcceptor {
    // TODO
}

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
    type Item = stream::Result<PeerStream>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let _ = cx;
        todo!()
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
    pub async fn accept(&mut self) -> stream::Result<BidirectionalStream> {
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
    ) -> core::task::Poll<crate::stream::Result<BidirectionalStream>> {
        let _ = cx;
        todo!()
    }
}

impl futures::stream::Stream for BidirectionalStreamAcceptor {
    type Item = stream::Result<BidirectionalStream>;

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
    pub async fn accept(&mut self) -> stream::Result<ReceiveStream> {
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
    ) -> core::task::Poll<stream::Result<ReceiveStream>> {
        let _ = cx;
        todo!()
    }
}

impl futures::stream::Stream for ReceiveStreamAcceptor {
    type Item = stream::Result<ReceiveStream>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let _ = cx;
        todo!()
    }
}
