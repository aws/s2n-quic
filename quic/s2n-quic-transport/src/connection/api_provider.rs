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
    stream::{ops, StreamId, StreamType},
};

/// A dynamically dispatched connection API
pub(crate) type ConnectionApi = Arc<dyn ConnectionApiProvider>;

/// The trait for types which provide the public Connection and Stream API via
/// dynamic dispatch
pub(crate) trait ConnectionApiProvider: Sync + Send {
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

    fn sni(&self) -> Option<Bytes>;

    fn alpn(&self) -> Bytes;

    fn id(&self) -> u64;

    fn ping(&self) -> Result<(), connection::Error>;
}
