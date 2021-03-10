// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines state which is shared between the public Stream and Connection API
//! and the internal Connection representation

use crate::{
    connection::{self, ConnectionApi, ConnectionApiProvider, InternalConnectionId},
    contexts::ConnectionApiCallContext,
    endpoint,
    space::PacketSpaceManager,
    stream::{AbstractStreamManager, Stream, StreamError},
    wakeup_queue::WakeupHandle,
};
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::{
    application::ApplicationErrorCode,
    stream::{ops, StreamId, StreamType},
};
use std::sync::{Mutex, MutexGuard};

/// A synchronized version of all connection state which is shared between
/// the QUIC packet thread and application threads
pub struct SynchronizedSharedConnectionState<ConnectionConfigType: connection::Config> {
    inner: Mutex<SharedConnectionState<ConnectionConfigType>>,
}

impl<ConnectionConfigType: connection::Config>
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
            &mut AbstractStreamManager<ConnectionConfigType::Stream>,
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
            .0
            .stream_manager;

        func(stream_id, stream_manager, &mut api_call_context)
    }
}

/// Contains all connection state which is shared between the QUIC packet thread
/// and application threads
pub struct SharedConnectionState<Config: endpoint::Config> {
    pub space_manager: PacketSpaceManager<Config>,
    pub wakeup_handle: WakeupHandle<InternalConnectionId>,
}

impl<ConnectionConfigType: connection::Config> SharedConnectionState<ConnectionConfigType> {
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

impl<ConnectionConfigType: connection::Config> ConnectionApiProvider
    for SynchronizedSharedConnectionState<ConnectionConfigType>
{
    fn poll_request(
        &self,
        stream_id: StreamId,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        self.perform_api_call(stream_id, |stream_id, api, api_call_context| {
            api.poll_request(stream_id, api_call_context, request, context)
        })
    }

    fn poll_accept(
        &self,
        arc_self: &ConnectionApi,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<Stream>, connection::Error>> {
        let mut shared_state = self.lock();

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Application space must be available on active connections")
            .0
            .stream_manager;

        match stream_manager.poll_accept(stream_type, context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Err(e).into(),
            Poll::Ready(Ok(None)) => Ok(None).into(),
            Poll::Ready(Ok(Some(stream_id))) => {
                // Unlock the Mutex
                drop(shared_state);

                let stream = Stream::new(arc_self.clone(), stream_id);

                Ok(Some(stream)).into()
            }
        }
    }

    fn poll_open_stream(
        &self,
        arc_self: &ConnectionApi,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<Result<Stream, connection::Error>> {
        let mut shared_state = self.lock();

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Application space must be available on active connections")
            .0
            .stream_manager;

        match stream_manager.poll_open(stream_type, context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Err(e).into(),
            Poll::Ready(Ok(stream_id)) => {
                // Unlock the Mutex
                drop(shared_state);

                let stream = Stream::new(arc_self.clone(), stream_id);

                Ok(stream).into()
            }
        }
    }

    fn close_connection(&self, error_code: ApplicationErrorCode) {
        let mut shared_state = self.lock();

        let application_space = match shared_state.space_manager.application_mut() {
            Some((space, _handshake_status)) => space,
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

    fn sni(&self) -> Option<Bytes> {
        self.lock().space_manager.application()?.sni.clone()
    }

    fn alpn(&self) -> Bytes {
        self.lock()
            .space_manager
            .application()
            .expect("Application space must be available on active connections")
            .alpn
            .clone()
    }

    fn ping(&self) -> Result<(), connection::Error> {
        let mut shared_state = self.lock();

        let space = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Application space must be available on active connections")
            .0;

        if let Some(error) = space.stream_manager.close_reason() {
            return Err(error);
        }

        space.ping();

        // Notify the connection it needs to send a packet
        shared_state.wakeup_handle.wakeup();

        Ok(())
    }
}
