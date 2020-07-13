//! Defines state which is shared between the public Stream and Connection API
//! and the internal Connection representation

use crate::{
    connection::{ConnectionApi, ConnectionApiProvider, ConnectionConfig, InternalConnectionId},
    contexts::ConnectionApiCallContext,
    space::PacketSpaceManager,
    stream::{AbstractStreamManager, Stream, StreamError},
    wakeup_queue::WakeupHandle,
};
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::{
    application::ApplicationErrorCode,
    stream::{StreamId, StreamType},
};
use std::sync::{Mutex, MutexGuard};

/// A synchronized version of all connection state which is shared between
/// the QUIC packet thread and application threads
pub struct SynchronizedSharedConnectionState<ConnectionConfigType: ConnectionConfig> {
    inner: Mutex<SharedConnectionState<ConnectionConfigType>>,
}

impl<ConnectionConfigType: ConnectionConfig>
    SynchronizedSharedConnectionState<ConnectionConfigType>
{
    /// Creates a new shared state for the Connection
    pub fn new(
        space_manager: PacketSpaceManager<ConnectionConfigType>,
        wakeup_handle: WakeupHandle<InternalConnectionId>,
    ) -> Self {
        Self {
            inner: Mutex::new(SharedConnectionState::new(space_manager, wakeup_handle)),
        }
    }

    /// Locks the shared state of the connection, and returns a guard that allows
    /// to modify the shared state
    pub fn lock(&self) -> MutexGuard<'_, SharedConnectionState<ConnectionConfigType>> {
        self.inner
            .lock()
            .expect("Locking can only fail if locks are poisoned")
    }

    /// A helper method for performing an API call.
    ///
    /// It extracts the [`StreamManager`] and the [`WakeupHandle`] from the shared state, and allows
    /// to call a method using those.
    fn perform_api_call<F, R>(&self, stream_id: StreamId, func: F) -> R
    where
        F: FnOnce(
            StreamId,
            &mut AbstractStreamManager<ConnectionConfigType::StreamType>,
            &mut ConnectionApiCallContext,
        ) -> R,
    {
        let shared_state = &mut *self.lock();
        let mut api_call_context =
            ConnectionApiCallContext::from_wakeup_handle(&mut shared_state.wakeup_handle);

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Stream manager must be available")
            .stream_manager;

        func(stream_id, stream_manager, &mut api_call_context)
    }
}

/// Contains all connection state which is shared between the QUIC packet thread
/// and application threads
pub struct SharedConnectionState<ConnectionConfigType: ConnectionConfig> {
    pub space_manager: PacketSpaceManager<ConnectionConfigType>,
    pub wakeup_handle: WakeupHandle<InternalConnectionId>,
}

impl<ConnectionConfigType: ConnectionConfig> SharedConnectionState<ConnectionConfigType> {
    /// Creates a new shared state for the Connection
    pub fn new(
        space_manager: PacketSpaceManager<ConnectionConfigType>,
        wakeup_handle: WakeupHandle<InternalConnectionId>,
    ) -> Self {
        Self {
            space_manager,
            wakeup_handle,
        }
    }
}

impl<ConnectionConfigType: ConnectionConfig> ConnectionApiProvider
    for SynchronizedSharedConnectionState<ConnectionConfigType>
{
    fn poll_pop(
        &self,
        stream_id: StreamId,
        context: &Context,
    ) -> Poll<Result<Option<Bytes>, StreamError>> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.poll_pop(stream_id, api_call_context, context)
        })
    }

    fn stop_sending(
        &self,
        stream_id: StreamId,
        error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.stop_sending(stream_id, api_call_context, error_code)
        })
    }

    fn poll_push(
        &self,
        stream_id: StreamId,
        data: Bytes,
        context: &Context,
    ) -> Poll<Result<(), StreamError>> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.poll_push(stream_id, api_call_context, data, context)
        })
    }

    fn poll_finish(&self, stream_id: StreamId, context: &Context) -> Poll<Result<(), StreamError>> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.poll_finish(stream_id, api_call_context, context)
        })
    }

    fn reset(
        &self,
        stream_id: StreamId,
        error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.reset(stream_id, api_call_context, error_code)
        })
    }

    fn poll_accept(
        &self,
        arc_self: &ConnectionApi,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, StreamError>> {
        let mut shared_state = self.lock();

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Application space must be available on active connections")
            .stream_manager;

        let poll_result = stream_manager.poll_accept(stream_type, context);

        // Unlock the Mutex
        drop(shared_state);

        match poll_result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(internal_id)) => {
                Poll::Ready(Ok(Stream::new(arc_self.clone(), internal_id)))
            }
        }
    }

    fn close_connection(&self, error_code: ApplicationErrorCode) {
        let mut shared_state = self.lock();

        let application_space = match shared_state.space_manager.application_mut() {
            Some(space) => space,
            None => return,
        };

        let stream_manager = &mut application_space.stream_manager;
        if stream_manager.close_reason().is_some() {
            // The connection was already closed. We return early here to avoid
            // an unnecessary wakeup for the shared state.
            return;
        }

        stream_manager.close(error_code.into());

        // Wake up the Connection so that it gets aware about the close request.
        // So far we only reset the Streams, but we didn't have the chance to change
        // the Connections state since this is outside of the shared state.
        // TODO: This is a race. The Connection thinks it is still in the Active
        // state and might handle packets. However the StreamManager is closed.
        // The behavior of this will depend on the frame.
        shared_state.wakeup_handle.wakeup();
    }
}
