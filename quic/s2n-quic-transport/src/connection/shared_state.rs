// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines state which is shared between the public Stream and Connection API
//! and the internal Connection representation

use crate::{
    connection::{self, ConnectionApi, ConnectionApiProvider, InternalConnectionId},
    contexts::ConnectionApiCallContext,
    endpoint,
    space::PacketSpaceManager,
    stream::{Stream, StreamError},
    wakeup_queue::WakeupHandle,
};
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::{
    application,
    stream::{ops, StreamId, StreamType},
    transport,
};
use std::sync::{Mutex, MutexGuard};

/// A synchronized version of all connection state which is shared between
/// the QUIC packet thread and application threads
pub struct SynchronizedSharedConnectionState<EndpointConfig: endpoint::Config> {
    inner: Mutex<SharedConnectionState<EndpointConfig>>,
}

impl<EndpointConfig: endpoint::Config> SynchronizedSharedConnectionState<EndpointConfig> {
    /// Creates a new shared state for the Connection
    pub fn new(
        space_manager: PacketSpaceManager<EndpointConfig>,
        wakeup_handle: WakeupHandle<InternalConnectionId>,
        internal_connection_id: InternalConnectionId,
    ) -> Self {
        Self {
            inner: Mutex::new(SharedConnectionState::new(
                space_manager,
                wakeup_handle,
                internal_connection_id,
            )),
        }
    }

    /// Locks the shared state of the connection, and returns a guard that allows
    /// to modify the shared state
    pub fn try_lock_error(
        &self,
    ) -> Result<MutexGuard<'_, SharedConnectionState<EndpointConfig>>, connection::Error> {
        let state = self.try_lock()?;

        if let Some(error) = state.error {
            return Err(error);
        }

        Ok(state)
    }

    /// Locks the shared state of the connection, and returns a guard that allows
    /// to modify the shared state
    pub fn lock(&self) -> MutexGuard<'_, SharedConnectionState<EndpointConfig>> {
        self.try_lock().expect("shared state has been poisoned")
    }

    /// Locks the shared state of the connection, and returns a guard that allows
    /// to modify the shared state
    pub fn try_lock(
        &self,
    ) -> Result<MutexGuard<'_, SharedConnectionState<EndpointConfig>>, connection::Error> {
        self.inner
            .lock()
            .map_err(|_| connection::Error::Unspecified)
    }
}

/// Contains all connection state which is shared between the QUIC packet thread
/// and application threads
pub struct SharedConnectionState<Config: endpoint::Config> {
    pub space_manager: PacketSpaceManager<Config>,
    pub wakeup_handle: WakeupHandle<InternalConnectionId>,
    pub internal_connection_id: InternalConnectionId,
    pub error: Option<connection::Error>,
}

impl<EndpointConfig: endpoint::Config> SharedConnectionState<EndpointConfig> {
    /// Creates a new shared state for the Connection
    pub fn new(
        space_manager: PacketSpaceManager<EndpointConfig>,
        wakeup_handle: WakeupHandle<InternalConnectionId>,
        internal_connection_id: InternalConnectionId,
    ) -> Self {
        Self {
            space_manager,
            wakeup_handle,
            internal_connection_id,
            error: None,
        }
    }
}

impl<EndpointConfig: endpoint::Config> ConnectionApiProvider
    for SynchronizedSharedConnectionState<EndpointConfig>
{
    fn poll_request(
        &self,
        stream_id: StreamId,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        let shared_state = &mut *self.try_lock_error()?;
        let mut api_call_context =
            ConnectionApiCallContext::from_wakeup_handle(&mut shared_state.wakeup_handle);

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Stream manager must be available")
            .0
            .stream_manager;

        stream_manager.poll_request(stream_id, &mut api_call_context, request, context)
    }

    fn poll_accept(
        &self,
        arc_self: &ConnectionApi,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<Stream>, connection::Error>> {
        // the stream manager has it's own check
        let mut shared_state = self.try_lock()?;

        let error = shared_state.error;

        let stream_manager = &mut shared_state
            .space_manager
            .application_mut()
            .ok_or_else(|| error.unwrap_or(connection::Error::Unspecified))?
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
        let mut shared_state = self.try_lock_error()?;

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

    fn close_connection(&self, error: Option<application::Error>) {
        if let Ok(mut shared_state) = self.try_lock() {
            let error = if let Some(error) = error {
                connection::Error::Application {
                    error,
                    initiator: endpoint::Location::Local,
                }
            } else {
                transport::Error::APPLICATION_ERROR.into()
            };

            shared_state.error = Some(error);

            // notify the stream_manager if applicable
            if let Some((application, _)) = shared_state.space_manager.application_mut() {
                application.stream_manager.close(error);
            }

            // Wake up the Connection so that it knows about the close request.
            shared_state.wakeup_handle.wakeup();
        }
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

    fn id(&self) -> u64 {
        self.lock().internal_connection_id.into()
    }

    fn ping(&self) -> Result<(), connection::Error> {
        let mut shared_state = self.try_lock_error()?;

        let space = &mut shared_state
            .space_manager
            .application_mut()
            .expect("Application space must be available on active connections")
            .0;

        space.ping();

        // Notify the connection it needs to send a packet
        shared_state.wakeup_handle.wakeup();

        Ok(())
    }
}
