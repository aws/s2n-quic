// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the trait for types which provide the public Connection and Stream API
//! via dynamic dispatch

use crate::{
    connection,
    stream::{Stream, StreamError},
};
use alloc::sync::Arc;
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::{
    application,
    application::Sni,
    event::query::{Query, QueryMut},
    inet::SocketAddress,
    stream::{ops, StreamId, StreamType},
};
use std::sync::atomic::AtomicUsize;

/// A dynamically dispatched connection API
pub(crate) type ConnectionApi = Arc<dyn ConnectionApiProvider>;

/// The trait for types which provide the public Connection and Stream API via
/// dynamic dispatch
pub(crate) trait ConnectionApiProvider: Sync + Send {
    fn application_handle_count(&self) -> &AtomicUsize;

    fn poll_request(
        &self,
        stream_id: StreamId,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError>;

    fn poll_accept(
        &self,
        arc_self: &Arc<dyn ConnectionApiProvider>,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<Stream>, connection::Error>>;

    fn poll_open_stream(
        &self,
        arc_self: &Arc<dyn ConnectionApiProvider>,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, connection::Error>>;

    fn close_connection(&self, code: Option<application::Error>);

    fn sni(&self) -> Result<Option<Sni>, connection::Error>;

    fn alpn(&self) -> Result<Bytes, connection::Error>;

    fn id(&self) -> u64;

    fn ping(&self) -> Result<(), connection::Error>;

    fn local_address(&self) -> Result<SocketAddress, connection::Error>;

    fn remote_address(&self) -> Result<SocketAddress, connection::Error>;

    fn query_event_context(&self, query: &mut dyn Query) -> Result<(), connection::Error>;

    fn query_event_context_mut(&self, query: &mut dyn QueryMut) -> Result<(), connection::Error>;
}
