// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_handle_api {
    (| $handle:ident, $dispatch:ident | $dispatch_body:expr) => {
        /// Opens a new [`LocalStream`](`crate::stream::LocalStream`) with a specific type
        ///
        /// The method will return
        ///  - `Ok(stream)` if a stream of the requested type was opened
        ///  - `Err(stream_error)` if the stream could not be opened due to an error
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::connection::Result<()> {
        /// #   use s2n_quic::stream;
        /// #   let mut handle: s2n_quic::connection::Handle = todo!();
        /// #
        /// while let Ok(stream) = handle.open_stream(stream::Type::Bidirectional).await {
        ///     println!("Stream opened from {:?}", stream.connection().remote_addr());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn open_stream(
            &mut self,
            stream_type: $crate::stream::Type,
        ) -> $crate::connection::Result<$crate::stream::LocalStream> {
            futures::future::poll_fn(|cx| self.poll_open_stream(stream_type, cx)).await
        }

        /// Polls opening a [`LocalStream`](`crate::stream::LocalStream`) with a specific type
        ///
        /// The method will return
        /// - `Poll::Ready(Ok(stream))` if a stream of the requested type was opened
        /// - `Poll::Ready(Err(stream_error))` if the stream could not be opened due to an error
        /// - `Poll::Pending` if the stream has not been opened yet
        #[inline]
        pub fn poll_open_stream(
            &mut self,
            stream_type: $crate::stream::Type,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::LocalStream>> {
            s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                use s2n_quic_core::stream::StreamType;
                use $crate::stream::{BidirectionalStream, SendStream};

                Ok(
                    match core::task::ready!(self.0.poll_open_stream(stream_type, cx))? {
                        stream if stream_type == StreamType::Unidirectional => {
                            SendStream::new(stream.into()).into()
                        }
                        stream => BidirectionalStream::new(stream).into(),
                    },
                )
                .into()
            })
        }

        /// Opens a new [`BidirectionalStream`](`crate::stream::BidirectionalStream`)
        ///
        /// The method will return
        ///  - `Ok(stream)` if a bidirectional stream was opened
        ///  - `Err(stream_error)` if the stream could not be opened due to an error
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::connection::Result<()> {
        /// #   let mut handle: s2n_quic::connection::Handle = todo!();
        /// #
        /// while let Ok(mut stream) = handle.open_bidirectional_stream().await {
        ///     println!("Stream opened from {:?}", stream.connection().remote_addr());
        /// }
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn open_bidirectional_stream(
            &mut self,
        ) -> $crate::connection::Result<$crate::stream::BidirectionalStream> {
            futures::future::poll_fn(|cx| self.poll_open_bidirectional_stream(cx)).await
        }

        /// Polls opening a [`BidirectionalStream`](`crate::stream::BidirectionalStream`)
        ///
        /// The method will return
        /// - `Poll::Ready(Ok(stream))` if a bidirectional stream was opened
        /// - `Poll::Ready(Err(stream_error))` if the stream could not be opened due to an error
        /// - `Poll::Pending` if the stream has not been opened yet
        #[inline]
        pub fn poll_open_bidirectional_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::BidirectionalStream>> {
            s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                use s2n_quic_core::stream::StreamType;
                use $crate::stream::BidirectionalStream;

                let stream =
                    core::task::ready!(self.0.poll_open_stream(StreamType::Bidirectional, cx))?;

                Ok(BidirectionalStream::new(stream)).into()
            })
        }

        /// Opens a [`SendStream`](`crate::stream::SendStream`)
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::connection::Result<()> {
        /// #   let mut connection: s2n_quic::connection::Handle = todo!();
        /// #
        /// let stream = connection.open_send_stream().await?;
        /// println!("Send stream opened with id: {}", stream.id());
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub async fn open_send_stream(
            &mut self,
        ) -> $crate::connection::Result<$crate::stream::SendStream> {
            futures::future::poll_fn(|cx| self.poll_open_send_stream(cx)).await
        }

        /// Polls opening a [`SendStream`](`crate::stream::SendStream`)
        #[inline]
        pub fn poll_open_send_stream(
            &mut self,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<$crate::connection::Result<$crate::stream::SendStream>> {
            s2n_quic_core::task::waker::debug_assert_contract(cx, |cx| {
                use s2n_quic_core::stream::StreamType;
                use $crate::stream::SendStream;

                let stream =
                    core::task::ready!(self.0.poll_open_stream(StreamType::Unidirectional, cx))?;

                Ok(SendStream::new(stream.into())).into()
            })
        }

        /// Returns the local address that this connection is bound to.
        #[inline]
        pub fn local_addr(&self) -> $crate::connection::Result<std::net::SocketAddr> {
            self.0.local_address().map(std::net::SocketAddr::from)
        }

        /// Returns the remote address that this connection is connected to.
        #[inline]
        pub fn remote_addr(&self) -> $crate::connection::Result<std::net::SocketAddr> {
            self.0.remote_address().map(std::net::SocketAddr::from)
        }

        /// Returns the negotiated server name the connection is using.
        #[inline]
        pub fn server_name(&self) -> $crate::connection::Result<Option<$crate::server::Name>> {
            self.0.server_name()
        }

        /// Returns the negotiated application protocol the connection is using
        #[inline]
        pub fn application_protocol(&self) -> $crate::connection::Result<::bytes::Bytes> {
            self.0.application_protocol()
        }

        /// Returns the application context the connection is using.
        #[inline]
        pub fn take_tls_context(
            &mut self,
        ) -> Option<std::boxed::Box<dyn core::any::Any + Send + Sync>> {
            self.0.take_tls_context()
        }

        /// Returns the internal identifier for the [`Connection`](`crate::Connection`)
        ///
        /// Note: This internal identifier is not the same as the connection ID included in packet
        /// headers as described in [QUIC Transport RFC](https://www.rfc-editor.org/rfc/rfc9000.html#name-connection-id)
        #[inline]
        pub fn id(&self) -> u64 {
            self.0.id()
        }

        /// Sends a Ping frame to the peer
        #[inline]
        pub fn ping(&mut self) -> $crate::connection::Result<()> {
            self.0.ping()
        }

        /// Enables or disables the connection to actively keep the connection alive with the peer
        ///
        /// This can be useful for maintaining connections beyond the configured idle timeout. The
        /// connection will continue to be held open until the keep alive is disabled or the
        /// connection is no longer able to be maintained due to connectivity.
        #[inline]
        pub fn keep_alive(&mut self, enabled: bool) -> $crate::connection::Result<()> {
            self.0.keep_alive(enabled)
        }

        /// Closes the Connection with the provided error code
        ///
        /// This will immediately terminate all outstanding streams.
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # async fn test() -> s2n_quic::connection::Result<()> {
        /// #   let mut connection: s2n_quic::connection::Handle = todo!();
        /// #
        /// const MY_ERROR_CODE:u32 = 99;
        /// connection.close(MY_ERROR_CODE.into());
        /// #
        /// #   Ok(())
        /// # }
        /// ```
        #[inline]
        pub fn close(&self, error_code: $crate::application::Error) {
            self.0.close(error_code)
        }

        /// API for querying the connection's
        /// [`Subscriber::ConnectionContext`](crate::provider::event::Subscriber::ConnectionContext).
        ///
        /// The ConnectionContext provides a mechanism for users to provide a custom
        /// type and update it on each event. The query APIs (check
        /// [`Self::query_event_context_mut`] for mutable version) provide a way to inspect the
        /// ConnectionContext outside of events.
        ///
        /// This function takes a `FnOnce(&EventContext) -> Outcome`, where `EventContext`
        /// represents the type of `ConnectionContext`. If the `EventContext` type matches
        /// any of the types of the configured Subscriber's context, the query is executed
        /// and `Ok(Outcome)` is returned, else
        /// `Err(`[`query::Error`](s2n_quic_core::query::Error)`)`.
        ///
        /// Given that it is possible to compose Subscriber, which can have different
        /// ConnectionContext types, this function traverses all Subscribers, executes
        /// and short-circuiting on the first match.
        ///
        /// # Examples
        ///
        /// ```no_run
        /// use s2n_quic::{provider::event::{events, query, Subscriber}, Connection, Server};
        ///
        /// struct MySubscriber{}
        ///
        /// impl Subscriber for MySubscriber {
        ///     type ConnectionContext = MyEventContext;
        ///     fn create_connection_context(
        ///        &mut self, _meta: &events::ConnectionMeta,
        ///        _info: &events::ConnectionInfo,
        ///     ) -> Self::ConnectionContext {
        ///         MyEventContext { request: 0 }
        ///     }
        ///  }
        ///
        /// #[derive(Clone, Copy)]
        /// pub struct MyEventContext {
        ///     request: u64,
        /// }
        ///
        /// let mut server = Server::builder()
        ///   .with_event(MySubscriber {}).unwrap()
        ///   .start().unwrap();
        /// # let connection: Connection = todo!();
        ///
        /// let outcome: Result<MyEventContext, query::Error> = connection
        ///     .query_event_context(|event_context: &MyEventContext| *event_context);
        ///
        /// match outcome {
        ///     Ok(event_context) => {
        ///         // `MyEventContext` matched a Subscriber::ConnectionContext and the
        ///         // query executed.
        ///         //
        ///         // use the value event_context for logging, etc..
        ///     }
        ///     Err(query::Error::ConnectionLockPoisoned) => {
        ///         // The query did not execute because of a connection error.
        ///         //
        ///         // log an error, panic, etc..
        ///     }
        ///     Err(query::Error::ContextTypeMismatch) => {
        ///         // `MyEventContext` failed to match any Subscriber::ConnectionContext
        ///         // and the query did not execute.
        ///         //
        ///         // log an error, panic, etc..
        ///     }
        ///     Err(_) => {
        ///         // We encountered an unknown error so handle it generically, e.g. log,
        ///         // panic, etc.
        ///     }
        /// }
        /// ```
        ///
        /// # Traverse order
        /// Let's demonstrate the traversal order for matching on ConnectionContext in the
        /// example below. We provide a composed Subscriber type (Foo, Bar), where both
        /// Foo and Bar have a ConnectionContext type of `u64`. The query traverse order
        /// is as follows:
        /// - `(Foo::ConnectionContext, Bar::ConnectionContext)`
        /// - `Foo::ConnectionContext`
        /// - `Bar::ConnectionContext`
        ///
        /// Note: In this example the type `u64` will always match `Foo::u64` and
        /// `Bar::u64` will never be matched. If this is undesirable, applications should
        /// make unique associated `ConnectionContext`s by creating new types.
        ///
        /// ```no_run
        /// use s2n_quic::{provider::event::{events, Subscriber}, Connection, Server};
        ///
        /// struct Foo {}
        ///
        /// impl Subscriber for Foo {
        ///    type ConnectionContext = u64;
        ///    fn create_connection_context(
        ///        &mut self, _meta: &events::ConnectionMeta,
        ///        _info: &events::ConnectionInfo,
        ///    ) -> Self::ConnectionContext { 0 }
        /// }
        ///
        /// struct Bar {}
        ///
        /// impl Subscriber for Bar {
        ///    type ConnectionContext = u64;
        ///    fn create_connection_context(
        ///        &mut self, _meta: &events::ConnectionMeta,
        ///        _info: &events::ConnectionInfo,
        ///    ) -> Self::ConnectionContext { 0 }
        /// }
        ///
        /// let mut server = Server::builder()
        ///     .with_event((Foo {}, Bar {})).unwrap()
        ///     .start().unwrap();
        /// # let connection: Connection = todo!();
        ///
        /// // Matches Foo.
        /// //
        /// // Note: Because the `ConnectionContext` type is the same for
        /// // both `Foo` and `Bar`, only `Foo`'s context will be matched.
        /// let _ = connection.query_event_context(|ctx: &u64| *ctx );
        ///
        /// // Matches (Foo, Bar).
        /// let _ = connection.query_event_context(|ctx: &(u64, u64)| ctx.0 );
        /// ```
        pub fn query_event_context<Query, EventContext, Outcome>(
            &self,
            query: Query,
        ) -> core::result::Result<Outcome, s2n_quic_core::query::Error>
        where
            Query: FnOnce(&EventContext) -> Outcome,
            EventContext: 'static,
        {
            use s2n_quic_core::query;
            let mut query = query::Once::new(query);

            self.0
                .query_event_context(&mut query)
                .map_err(|_| query::Error::ConnectionLockPoisoned)?;

            query.into()
        }

        /// API for querying the connection's
        /// [`Subscriber::ConnectionContext`](crate::provider::event::Subscriber::ConnectionContext).
        ///
        /// Similar to [`Self::query_event_context`] but provides
        /// mutable access to `ConnectionContext`.
        ///
        /// ```ignore
        /// let outcome = connection
        ///     .query_event_context(
        ///         |event_context: &MyEventContext| event_context.request += 1
        ///     );
        /// ```
        pub fn query_event_context_mut<Query, EventContext, Outcome>(
            &mut self,
            query: Query,
        ) -> core::result::Result<Outcome, s2n_quic_core::query::Error>
        where
            Query: FnOnce(&mut EventContext) -> Outcome,
            EventContext: 'static,
        {
            use s2n_quic_core::query;
            let mut query = query::Once::new_mut(query);

            self.0
                .query_event_context_mut(&mut query)
                .map_err(|_| query::Error::ConnectionLockPoisoned)?;

            query.into()
        }

        /// API for querying the connection's datagram endpoint.
        ///
        ///  Provides mutable access to `Sender` or `Receiver`.
        ///
        /// ```ignore
        /// let outcome = connection
        ///     .datagram_mut(
        ///         |sender: &MySender| sender.send_datagram(Bytes::from_static(&[1, 2, 3]));
        ///     );
        /// ```
        pub fn datagram_mut<Query, ProviderType, Outcome>(
            &self,
            query: Query,
        ) -> core::result::Result<Outcome, s2n_quic_core::query::Error>
        where
            Query: FnOnce(&mut ProviderType) -> Outcome,
            ProviderType: 'static,
        {
            use s2n_quic_core::query;
            let mut query = query::Once::new_mut(query);

            self.0
                .datagram_mut(&mut query)
                .map_err(|_| query::Error::ConnectionLockPoisoned)?;

            query.into()
        }
    };
}

#[derive(Clone, Debug)]
pub struct Handle(pub(crate) s2n_quic_transport::connection::Connection);

impl Handle {
    impl_handle_api!(|handle, call| call!(handle));
}
