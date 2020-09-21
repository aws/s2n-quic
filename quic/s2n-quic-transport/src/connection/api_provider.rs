//! Defines the trait for types which provide the public Connection and Stream API
//! via dynamic dispatch

use crate::stream::{Stream, StreamError};
use alloc::sync::Arc;
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::{
    application::ApplicationErrorCode,
    stream::{StreamId, StreamType},
};

/// A dynamically dispatched connection API
pub(crate) type ConnectionApi = Arc<dyn ConnectionApiProvider>;

/// The trait for types which provide the public Connection and Stream API via
/// dynamic dispatch
pub(crate) trait ConnectionApiProvider: Sync + Send {
    fn poll_pop(
        &self,
        stream_id: StreamId,
        context: &Context,
    ) -> Poll<Result<Option<Bytes>, StreamError>>;

    fn stop_sending(
        &self,
        stream_id: StreamId,
        error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError>;

    fn poll_push(
        &self,
        stream_id: StreamId,
        data: Bytes,
        context: &Context,
    ) -> Poll<Result<(), StreamError>>;

    fn poll_finish(&self, stream_id: StreamId, context: &Context) -> Poll<Result<(), StreamError>>;

    fn reset(
        &self,
        stream_id: StreamId,
        error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError>;

    fn poll_accept(
        &self,
        arc_self: &Arc<dyn ConnectionApiProvider>,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<(Stream, StreamType), StreamError>>;

    fn close_connection(&self, error_code: ApplicationErrorCode);
}
