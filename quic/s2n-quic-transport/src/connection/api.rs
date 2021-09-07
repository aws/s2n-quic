// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the public QUIC connection API

use crate::{
    connection::{self, ConnectionApi},
    stream::{ops, Stream, StreamError, StreamId},
};
use bytes::Bytes;
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_core::{
    application,
    application::Sni,
    event::query::{Query, QueryMut},
    inet::SocketAddress,
    stream::StreamType,
};

/// A QUIC connection
#[derive(Clone)]
pub struct Connection {
    /// The inner connection API implementation
    ///
    /// This uses a dynamically-dispatched interface to hide all of the connection's
    /// generic parameters and allows applications to interact with connections in a
    /// straightforward manner.
    pub(super) api: ConnectionApi,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO print interesting virtual fields
        f.debug_struct("Connection").finish()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // If the connection wasn't closed before, close it now to make sure
        // all Streams terminate.
        self.api.close_connection(None);
    }
}

impl Connection {
    pub(crate) fn new(api: ConnectionApi) -> Self {
        Self { api }
    }

    /// Accepts an incoming [`Stream`]
    ///
    /// The method will return
    /// - `Poll::Ready(Ok(Some(stream, stream_type)))` if a [`Stream`] was accepted
    /// - `Poll::Ready(Ok(None))` if the connection was closed without an error
    /// - `Poll::Ready(Err(stream_error))` if no could be accepted due to an error
    /// - `Poll::Pending` if no new [`Stream`] of the given type was accepted by the connection yet.
    ///   In this case the caller must retry calling [`Self::poll_accept`].
    ///   For this purpose the method will save the [`core::task::Waker`]
    ///   which is provided as part of the [`Context`] parameter, and notify it
    ///   as soon as retrying the method will yield a different result.
    #[inline]
    pub fn poll_accept(
        &mut self,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<Stream>, connection::Error>> {
        self.api.poll_accept(&self.api, stream_type, context)
    }

    #[inline]
    pub fn poll_open_stream(
        &mut self,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, connection::Error>> {
        self.api.poll_open_stream(&self.api, stream_type, context)
    }

    #[inline]
    pub fn poll_request(
        &self,
        stream_id: StreamId,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        self.api.poll_request(stream_id, request, context)
    }

    /// Closes the Connection with the provided error code
    ///
    /// This will immediatly terminate all outstanding streams.
    #[inline]
    pub fn close(&self, error_code: application::Error) {
        self.api.close_connection(Some(error_code));
    }

    #[inline]
    pub fn sni(&self) -> Result<Option<Sni>, connection::Error> {
        self.api.sni()
    }

    #[inline]
    pub fn alpn(&self) -> Result<Bytes, connection::Error> {
        self.api.alpn()
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.api.id()
    }

    #[inline]
    pub fn ping(&self) -> Result<(), connection::Error> {
        self.api.ping()
    }

    #[inline]
    pub fn local_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api.local_address()
    }

    #[inline]
    pub fn remote_address(&self) -> Result<SocketAddress, connection::Error> {
        self.api.remote_address()
    }

    #[inline]
    pub fn query_event_context(&self, query: &mut dyn Query) -> Result<(), connection::Error> {
        self.api.query_event_context(query)
    }

    #[inline]
    pub fn query_event_context_mut(
        &self,
        query: &mut dyn QueryMut,
    ) -> Result<(), connection::Error> {
        self.api.query_event_context_mut(query)
    }
}
