//! Defines the Stream objects that applications are interacting with

use crate::connection::ConnectionApi;
use bytes::Bytes;
use core::task::{Context, Poll};
pub use s2n_quic_core::{
    application::ApplicationErrorCode,
    stream::{StreamError, StreamId, StreamType},
};

struct StreamState {
    shared_state: ConnectionApi,
    stream_id: StreamId,
}

impl StreamState {
    fn new(shared_state: ConnectionApi, stream_id: StreamId) -> Self {
        Self {
            shared_state,
            stream_id,
        }
    }
}

macro_rules! send_apis {
    () => {
        /// Enqueues a chunk of data for sending it towards the peer. The method
        /// will return
        /// - `Poll::Ready(Ok())` if the data could be enqueued for sending.
        /// - `Poll::Ready(Err(stream_error))` if data could be enqueued, because
        ///   the `Stream` entered an error state. This will for example be the
        ///   case if the `Stream` had been previously reset.
        /// - `Poll::Pending` if the send buffer capacity is exhausted. In this case
        ///   the caller must retry enqueueing the data as soon as space in the send
        ///   queue frees up. For this purpose the method will save the [`Waker`]
        ///   which is provided as part of the [`Context`] parameter, and notify it
        ///   as soon as retrying the method will yield a different result.
        pub fn poll_push(
            &mut self,
            data: Bytes,
            context: &Context,
        ) -> Poll<Result<(), StreamError>> {
            self.state
                .shared_state
                .poll_push(self.state.stream_id, data, context)
        }

        /// Marks the `Stream` as finished, and allows to wait until all data and
        /// the finish acknowledgement had been received by the peer.
        ///
        /// The method will return
        /// - `Poll::Pending` if the `Stream` had been marked as finished, but
        ///   some data or the end of stream flag have not yet been acknowledged by
        ///   the peer. In this case the method will save the [`Waker`] which is
        ///   provided as part of the [`Context`] parameter, and notify it as soon
        ///   as all data had been acknowledged or if the `Stream` entered an error
        ///   state.
        /// - `Poll::Ready(Ok(()))`, as soon as all data and the end of stream flag
        ///   had been acknowledged by the peer. In this case the `Stream` has
        ///   entered its terminal state.
        /// - `Poll::Ready(Err(stream_error))` if the `Stream` can not be finished,
        ///   because it entered an error state. This can for example happen if the
        ///   `Stream` got reset locally, or through a `STOP_SENDING` message from
        ///   the peer.
        pub fn poll_finish(&mut self, context: &Context) -> Poll<Result<(), StreamError>> {
            self.state
                .shared_state
                .poll_finish(self.state.stream_id, context)
        }

        /// Initiates a `RESET` of the `Stream`
        ///
        /// This will trigger sending a `RESET` message to the peer, which will
        /// contain the given error code.
        pub fn reset(&mut self, error_code: ApplicationErrorCode) -> Result<(), StreamError> {
            self.state
                .shared_state
                .reset(self.state.stream_id, error_code)
        }
    };
}

macro_rules! receive_apis {
    () => {
        /// Reads a chunk of received data from the `Stream`.
        ///
        /// This will return
        /// - `Poll::Ready(Ok(Some(Bytes)))` if data is available for reading.
        /// - `Poll::Ready(Ok(None)) if all data on the `Stream` had been received
        ///   (end of Stream)
        /// - `Poll_Ready(Err(stream_error))`, if the `Stream` entered an error
        ///   state, and reading of data is no longer possible. This can for example
        ///   happen if the `Stream` got reset by the peer.
        /// - `Poll::Pending` if currently there is no data available for reading,
        ///   but the `Stream` has not finished yet. In this case the method will
        ///   copy the supplied `Waker` in the `Context` parameter, and notify it
        ///   as soon as data arrives.
        pub fn poll_pop(&mut self, context: &Context) -> Poll<Result<Option<Bytes>, StreamError>> {
            self.state
                .shared_state
                .poll_pop(self.state.stream_id, context)
        }

        /// Sends a `STOP_SENDING` message to the peer. This requests the peer to
        /// finish the `Stream` as soon as possible by issuing a `RESET` with the
        /// provided `error_code`.
        ///
        /// Since this is merely a request to the peer to `RESET` the `Stream`, the
        /// `Stream` will not immediately be in a `RESET` state after issuing this
        /// API call.
        ///
        /// If the `Stream` had been previously reset by the peer or if all data had
        /// already been received the API call will not trigger any action.
        pub fn stop_sending(
            &mut self,
            error_code: ApplicationErrorCode,
        ) -> Result<(), StreamError> {
            self.state
                .shared_state
                .stop_sending(self.state.stream_id, error_code)
        }
    };
}

/// A readable and writeable QUIC stream
pub struct Stream {
    state: StreamState,
    reset_on_drop: bool,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if self.reset_on_drop {
            // Reset the sending half of the `Stream`, for the case the
            // application did not reset it itself or sent all data.
            // If that already happened, this will be a noop.
            let _ = self.reset(ApplicationErrorCode::UNKNOWN);

            // Send a STOP_SENDING message on the receiving half of the `Stream`,
            // for the case the application did not consume all data.
            // If that already happened, this will be a noop.
            let _ = self.stop_sending(ApplicationErrorCode::UNKNOWN);
        }
    }
}

impl Stream {
    send_apis!();

    receive_apis!();

    /// Creates a `Stream` instance, which represents a QUIC stream with the
    /// given ID. All interactions with the `Stream` will be performed through
    /// the provided [`SynchronizedSharedConnectionState`].
    pub(crate) fn new(shared_state: ConnectionApi, stream_id: StreamId) -> Self {
        Self {
            state: StreamState::new(shared_state, stream_id),
            reset_on_drop: true,
        }
    }

    /// Splits a bidirectional QUIC Stream in two halves.
    ///
    /// One half can be used to read data from the Stream.
    /// The other half can be used to send data.
    pub fn split(mut self) -> (ReceiveStream, SendStream) {
        // This is not the most efficient implementation, since we bump and
        // decrease the refcount and need an extra field. However all
        // implementations which directly reuse `self.shared_state` by moving out of
        // it require a certain amount of unsafe code, since the `Stream::drop`
        // will by default be called at the end of this method.
        let readable_stream =
            ReceiveStream::new(self.state.shared_state.clone(), self.state.stream_id);
        let writable_stream =
            SendStream::new(self.state.shared_state.clone(), self.state.stream_id);

        self.reset_on_drop = false;
        (readable_stream, writable_stream)
    }
}

/// A writeable QUIC stream
pub struct SendStream {
    state: StreamState,
}

impl Drop for SendStream {
    fn drop(&mut self) {
        // Reset the `Stream`, for the case the application did not reset it
        // itself or sent all data. If that already happened, this will be a noop.
        let _ = self.reset(ApplicationErrorCode::UNKNOWN);
    }
}

impl SendStream {
    send_apis!();

    /// Creates a `Stream` instance, which represents a QUIC stream with the
    /// given ID. All interactions with the `Stream` will be performed through
    /// the provided [`SynchronizedSharedConnectionState`].
    pub(crate) fn new(shared_state: ConnectionApi, stream_id: StreamId) -> Self {
        Self {
            state: StreamState::new(shared_state, stream_id),
        }
    }
}

/// A readable QUIC stream
pub struct ReceiveStream {
    state: StreamState,
}

impl Drop for ReceiveStream {
    fn drop(&mut self) {
        // Send a STOP_SENDING message on the receiving half of the `Stream`,
        // for the case the application did not consume all data.
        // If that already happened, this will be a noop.
        let _ = self.stop_sending(ApplicationErrorCode::UNKNOWN);
    }
}

impl ReceiveStream {
    receive_apis!();

    /// Creates a `Stream` instance, which represents a QUIC stream with the
    /// given ID. All interactions with the `Stream` will be performed through
    /// the provided [`SynchronizedSharedConnectionState`].
    pub(crate) fn new(shared_state: ConnectionApi, stream_id: StreamId) -> Self {
        Self {
            state: StreamState::new(shared_state, stream_id),
        }
    }
}
