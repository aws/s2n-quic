//! Defines the public QUIC connection API

use crate::{
    connection::ConnectionApi,
    stream::{Stream, StreamError},
};
use core::task::{Context, Poll};
use s2n_quic_core::{application::ApplicationErrorCode, stream::StreamType};

/// A QUIC connection
pub struct Connection {
    /// The shared state, which contains the connections actual state
    shared_state: ConnectionApi,
}

impl Drop for Connection {
    fn drop(&mut self) {
        // If the connection wasn't closed before close it now to make sure
        // all Streams terminate.
        self.close(ApplicationErrorCode::UNKNOWN);
    }
}

impl Connection {
    pub(crate) fn new(shared_state: ConnectionApi) -> Self {
        Self { shared_state }
    }

    /// Accepts an incoming [`Stream`]
    ///
    /// The method will return
    /// - `Poll::Ready(Ok(stream))` if a [`Stream`] was accepted
    /// - `Poll::Ready(Err(stream_error))` if no could be accepted due to an error
    /// - `Poll::Pending` if no new [`Stream`] of the given type was accepted by the connection yet.
    ///   In this case the caller must retry calling [`poll_accept`].
    ///   For this purpose the method will save the [`Waker`]
    ///   which is provided as part of the [`Context`] parameter, and notify it
    ///   as soon as retrying the method will yield a different result.
    pub fn poll_accept(
        &mut self,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, StreamError>> {
        self.shared_state
            .poll_accept(&self.shared_state, stream_type, context)
    }

    /// Closes the Connection with the provided error code
    ///
    /// This will immediatly terminate all outstanding streams.
    pub fn close(&self, error_code: ApplicationErrorCode) {
        self.shared_state.close_connection(error_code);
    }
}
