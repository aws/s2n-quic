// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `StreamManager` manages the lifecycle of all `Stream`s inside a `Connection`

use crate::{
    connection,
    contexts::{ConnectionApiCallContext, OnTransmitError, WriteContext},
    recovery::RttEstimator,
    stream::{
        self,
        incoming_connection_flow_controller::IncomingConnectionFlowController,
        outgoing_connection_flow_controller::OutgoingConnectionFlowController,
        stream_container::{StreamContainer, StreamContainerIterationResult},
        stream_events::StreamEvents,
        stream_impl::StreamConfig,
        StreamError, StreamTrait,
    },
    transmission::{self, interest::Provider as _},
};
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use futures_core::ready;
use s2n_quic_core::{
    ack, endpoint,
    frame::{
        stream::StreamRef, DataBlocked, MaxData, MaxStreamData, MaxStreams, ResetStream,
        StopSending, StreamDataBlocked, StreamsBlocked,
    },
    packet::number::PacketNumberSpace,
    stream::{iter::StreamIter, ops, StreamId, StreamType},
    time::{timer, Timestamp},
    transport::{self, parameters::InitialFlowControlLimits},
    varint::VarInt,
};

/// Holds one Stream ID of each type (initiator/stream type)
#[derive(Debug)]
pub(super) struct StreamIdSet {
    server_initiated_unidirectional: Option<StreamId>,
    client_initiated_unidirectional: Option<StreamId>,
    server_initiated_bidirectional: Option<StreamId>,
    client_initiated_bidirectional: Option<StreamId>,
}

impl StreamIdSet {
    /// Returns the `StreamIdSet` where each `StreamId` is initialized to its
    /// initial value.
    pub fn initial() -> Self {
        Self {
            server_initiated_bidirectional: Some(StreamId::initial(
                endpoint::Type::Server,
                StreamType::Bidirectional,
            )),
            client_initiated_bidirectional: Some(StreamId::initial(
                endpoint::Type::Client,
                StreamType::Bidirectional,
            )),
            server_initiated_unidirectional: Some(StreamId::initial(
                endpoint::Type::Server,
                StreamType::Unidirectional,
            )),
            client_initiated_unidirectional: Some(StreamId::initial(
                endpoint::Type::Client,
                StreamType::Unidirectional,
            )),
        }
    }

    /// Returns the reference to the `StreamId` inside the set for the given
    /// initiator and stream type
    pub fn get_mut(
        &mut self,
        initiator: endpoint::Type,
        stream_type: StreamType,
    ) -> &mut Option<StreamId> {
        match (initiator, stream_type) {
            (endpoint::Type::Server, StreamType::Unidirectional) => {
                &mut self.server_initiated_unidirectional
            }
            (endpoint::Type::Client, StreamType::Unidirectional) => {
                &mut self.client_initiated_unidirectional
            }
            (endpoint::Type::Server, StreamType::Bidirectional) => {
                &mut self.server_initiated_bidirectional
            }
            (endpoint::Type::Client, StreamType::Bidirectional) => {
                &mut self.client_initiated_bidirectional
            }
        }
    }
}

/// Stores all required state for accepting incoming Streams via the
/// `accept()` method
#[derive(Debug)]
pub(super) struct AcceptState {
    /// The ID of the next bidirectional Stream that an `accept()` call
    /// should return.
    next_bidi_stream_to_accept: Option<StreamId>,
    /// The ID of the next unidirectional Stream that an `accept()` call
    /// should return.
    next_uni_stream_to_accept: Option<StreamId>,
    /// The `Waker` for the task which needs to get woken when the next
    /// bidirectional stream was accepted
    bidi_waker: Option<Waker>,
    /// The `Waker` for the task which needs to get woken when the next
    /// unidirectional stream was accepted
    uni_waker: Option<Waker>,
}

impl AcceptState {
    pub fn new(local_endpoint_type: endpoint::Type) -> AcceptState {
        let peer_type = local_endpoint_type.peer_type();

        AcceptState {
            next_bidi_stream_to_accept: Some(StreamId::initial(
                peer_type,
                StreamType::Bidirectional,
            )),
            next_uni_stream_to_accept: Some(StreamId::initial(
                peer_type,
                StreamType::Unidirectional,
            )),
            bidi_waker: None,
            uni_waker: None,
        }
    }

    /// Returns a mutable reference to the `Waker` for the given Stream type
    pub fn waker_mut(&mut self, stream_type: StreamType) -> &mut Option<Waker> {
        match stream_type {
            StreamType::Bidirectional => &mut self.bidi_waker,
            StreamType::Unidirectional => &mut self.uni_waker,
        }
    }

    /// Returns the ID of the next Stream that needs to get accepted through
    /// an `accept())` call.
    pub fn next_stream_id(&self, stream_type: StreamType) -> Option<StreamId> {
        match stream_type {
            StreamType::Bidirectional => self.next_bidi_stream_to_accept,
            StreamType::Unidirectional => self.next_uni_stream_to_accept,
        }
    }

    /// Returns a mutable reference to the ID of the next Stream that needs to
    /// get accepted through an `accept())` call.
    pub fn next_stream_mut(&mut self, stream_type: StreamType) -> &mut Option<StreamId> {
        match stream_type {
            StreamType::Bidirectional => &mut self.next_bidi_stream_to_accept,
            StreamType::Unidirectional => &mut self.next_uni_stream_to_accept,
        }
    }
}

/// Manages all active `Stream`s inside a connection
#[derive(Debug)]
pub struct StreamManagerState<S> {
    /// Flow control credit manager for receiving data
    pub(super) incoming_connection_flow_controller: IncomingConnectionFlowController,
    /// Flow control credit manager for sending data
    pub(super) outgoing_connection_flow_controller: OutgoingConnectionFlowController,
    /// Controller for managing streams concurrency limits
    stream_controller: stream::Controller,
    /// A container which contains all Streams
    streams: StreamContainer<S>,
    /// The next Stream ID which was not yet used for an initiated stream
    /// for each stream type
    pub(super) next_stream_ids: StreamIdSet,
    /// The type of our local endpoint (client or server)
    local_endpoint_type: endpoint::Type,
    /// The initial flow control limits which we advertised towards the peer
    /// via transport parameters
    initial_local_limits: InitialFlowControlLimits,
    /// The initial flow control limits we received from the peer via transport
    /// parameters
    initial_peer_limits: InitialFlowControlLimits,
    /// If the `StreamManager` was closed, this contains the error which was
    /// passed to the `close()` call
    close_reason: Option<connection::Error>,
    /// All state for accepting remotely initiated connections
    pub(super) accept_state: AcceptState,
    /// Limits for the Stream manager. Since only Stream limits are utilized at
    /// the moment we only store those
    stream_limits: stream::Limits,
}

impl<S: StreamTrait> StreamManagerState<S> {
    /// Performs the given transaction on the `StreamManagerState`.
    /// If an error occurs, all Streams will be reset with an internal reset.
    pub fn reset_streams_on_error<F, R>(&mut self, func: F) -> Result<R, transport::Error>
    where
        F: FnOnce(&mut Self) -> Result<R, transport::Error>,
    {
        let result = func(self);
        if let Err(err) = result.as_ref() {
            self.close((*err).into(), false);
        }
        result
    }

    /// Inserts the `Stream` into the StreamContainer.
    ///
    /// This method does not perform any validation whether it is allowed to
    /// open the `Stream`.
    fn insert_stream(&mut self, stream_id: StreamId) {
        // The receive window is announced by us towards to the peer
        let initial_receive_window = self
            .initial_local_limits
            .stream_limits
            .max_data(self.local_endpoint_type, stream_id);
        // The send window is announced to us by the peer
        let initial_send_window = self
            .initial_peer_limits
            .stream_limits
            .max_data(self.local_endpoint_type.peer_type(), stream_id);

        // We pass the initial_receive_window also as the desired flow control
        // window. Thereby we will maintain the same flow control window over
        // the lifetime of the Stream.
        // If we would want to have another limit, we would need to have various
        // limits for the various combinations of unidirectional/bidirectional
        // Streams. Those would bloat up the config, and essentially just
        // duplicate the transport parameters.

        // We limit the initial data limit to u32::MAX (4GB), which far
        // exceeds the reasonable amount of data a connection is
        // initially allowed to send.
        //
        // By representing the flow control value as a u32, we save space
        // on the connection state.
        assert!(
            initial_receive_window <= VarInt::from_u32(core::u32::MAX),
            "Receive window must not exceed 32bit range"
        );

        self.streams.insert_stream(S::new(StreamConfig {
            incoming_connection_flow_controller: self.incoming_connection_flow_controller.clone(),
            outgoing_connection_flow_controller: self.outgoing_connection_flow_controller.clone(),
            local_endpoint_type: self.local_endpoint_type,
            stream_id,
            initial_receive_window,
            desired_flow_control_window: initial_receive_window.as_u64() as u32,
            initial_send_window,
            max_send_buffer_size: self.stream_limits.max_send_buffer_size.as_u32(),
        }));
    }

    /// Opens a Stream which is referenced in a frame if it has not yet been
    /// opened so far. This will also open all unopened frames which a lower
    /// Stream ID of the same type, as required by the QUIC specification.
    fn open_stream_if_necessary(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        // If the stream ID is higher than any Stream ID we observed so far, we
        // need open all Stream IDs of the same type. Otherwise we need to look
        // up the Stream ID the map.

        let first_unopened_id: StreamId = if let Some(first_unopened_id) = *self
            .next_stream_ids
            .get_mut(stream_id.initiator(), stream_id.stream_type())
        {
            first_unopened_id
        } else {
            // All Streams for particular initiator end endpoint type have
            // already been opened. In this case we don't have to open a
            // Stream, and the referenced Stream ID can also not be higher
            // than a previous outgoing Stream ID we used.
            return Ok(());
        };

        if stream_id.initiator() != self.local_endpoint_type {
            if stream_id >= first_unopened_id {
                // This Stream ID is first referenced here. This means we have
                // to create a new Stream instance

                if self.close_reason.is_some() {
                    return Err(transport::Error::NO_ERROR.with_reason("Connection was closed"));
                }

                //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
                //# Endpoints MUST NOT exceed the limit set by their peer.  An endpoint
                //# that receives a frame with a stream ID exceeding the limit it has
                //# sent MUST treat this as a connection error of type
                //# STREAM_LIMIT_ERROR; see Section 11 for details on error handling.
                let stream_iter = StreamIter::new(first_unopened_id, stream_id);

                // Validate that there is enough capacity to open all streams.
                self.stream_controller.on_open_remote_stream(stream_iter)?;

                // We must create ALL streams with a lower Stream ID too:
                //
                //= https://www.rfc-editor.org/rfc/rfc9000#section-3.2
                //# Before a stream is created, all streams of the same type with lower-
                //# numbered stream IDs MUST be created.  This ensures that the creation
                //# order for streams is consistent on both endpoints.
                for stream_id in stream_iter {
                    self.insert_stream(stream_id);
                }

                //= https://www.rfc-editor.org/rfc/rfc9000#section-2.1
                //# A QUIC
                //# endpoint MUST NOT reuse a stream ID within a connection.

                // Increase the next expected Stream ID. We might thereby exhaust
                // the Stream ID range, which means we can no longer accept a
                // further Stream.
                *self
                    .next_stream_ids
                    .get_mut(stream_id.initiator(), stream_id.stream_type()) =
                    stream_id.next_of_type();

                // Wake up the application if it is waiting on new incoming Streams
                if let Some(waker) = self.accept_state.waker_mut(stream_id.stream_type()).take() {
                    waker.wake();
                }
            }
        } else {
            // Check if the peer is sending us a frame for a local initiated Stream with
            // a higher Stream ID than we ever used.
            // In this case the peer seems to be time-travelling and know about
            // Future Stream IDs we might use. We also will not accept this and
            // close the connection.
            if stream_id >= first_unopened_id {
                return Err(
                    transport::Error::STREAM_STATE_ERROR.with_reason("Stream was not yet opened")
                );
            }
        }

        Ok(())
    }

    fn poll_open_local_stream(
        &mut self,
        stream_type: StreamType,
        open_token: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<Result<StreamId, connection::Error>> {
        let first_unopened_id = self
            .next_stream_ids
            .get_mut(self.local_endpoint_type, stream_type)
            .ok_or_else(connection::Error::stream_id_exhausted)?;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
        //# Endpoints MUST NOT exceed the limit set by their peer.
        //
        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
        //# An endpoint MUST NOT open more streams than permitted by the current
        //# stream limit set by its peer.
        let poll_open =
            self.stream_controller
                .poll_open_local_stream(stream_type, open_token, context);

        // returns Pending if there is no capacity available
        ready!(poll_open);

        self.insert_stream(first_unopened_id);
        Poll::Ready(Ok(first_unopened_id))
    }

    fn close(&mut self, error: connection::Error, flush: bool) {
        if self.close_reason.is_some() {
            return;
        }
        self.close_reason = Some(error);

        self.streams
            .iterate_streams(&mut self.stream_controller, |stream| {
                // We have to wake inside the lock, since `StreamEvent`s has no capacity
                // to carry wakers in another iteration
                let mut events = StreamEvents::new();
                if flush {
                    stream.on_flush(error.into(), &mut events);
                } else {
                    stream.on_internal_reset(error.into(), &mut events);
                }
                events.wake_all();
            });

        // If the connection gets closed we need to notify tasks which are blocked
        // on `accept()`.

        if let Some(waker) = self
            .accept_state
            .waker_mut(StreamType::Bidirectional)
            .take()
        {
            waker.wake();
        }
        if let Some(waker) = self
            .accept_state
            .waker_mut(StreamType::Unidirectional)
            .take()
        {
            waker.wake();
        }

        self.stream_controller.close();
    }

    fn flush(&mut self, error: connection::Error) -> Poll<()> {
        self.close(error, true);

        // if we still have active streams, we're not done flushing
        if self.streams.nr_active_streams() > 0 {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

/// Manages all active `Stream`s inside a connection.
/// `AbstractStreamManager` is parameterized over the `Stream` type.
#[derive(Debug)]
pub struct AbstractStreamManager<S> {
    pub(super) inner: StreamManagerState<S>,
    last_blocked_sync_period: Duration,
}

// Sending the `AbstractStreamManager` between threads is safe, since we never expose the `Rc`s
// outside of the container
#[allow(unknown_lints, clippy::non_send_fields_in_send_ty)]
unsafe impl<S> Send for AbstractStreamManager<S> {}

impl<S: StreamTrait> AbstractStreamManager<S> {
    /// Creates a new `StreamManager` using the provided configuration parameters
    pub fn new(
        connection_limits: &connection::Limits,
        local_endpoint_type: endpoint::Type,
        initial_local_limits: InitialFlowControlLimits,
        initial_peer_limits: InitialFlowControlLimits,
    ) -> Self {
        // We limit the initial data limit to u32::MAX (4GB), which far
        // exceeds the reasonable amount of data a connection is
        // initially allowed to send.
        //
        // By representing the flow control value as a u32, we save space
        // on the connection state.
        assert!(
            initial_local_limits.max_data <= VarInt::from_u32(core::u32::MAX),
            "Receive window must not exceed 32bit range"
        );

        Self {
            inner: StreamManagerState {
                incoming_connection_flow_controller: IncomingConnectionFlowController::new(
                    initial_local_limits.max_data,
                    initial_local_limits.max_data.as_u64() as u32,
                ),
                outgoing_connection_flow_controller: OutgoingConnectionFlowController::new(
                    initial_peer_limits.max_data,
                ),
                stream_controller: stream::Controller::new(
                    local_endpoint_type,
                    initial_peer_limits,
                    initial_local_limits,
                    connection_limits.stream_limits(),
                ),
                streams: StreamContainer::new(),
                next_stream_ids: StreamIdSet::initial(),
                local_endpoint_type,
                initial_local_limits,
                initial_peer_limits,
                close_reason: None,
                accept_state: AcceptState::new(local_endpoint_type),
                stream_limits: connection_limits.stream_limits(),
            },
            last_blocked_sync_period: Duration::ZERO,
        }
    }

    /// The number of bytes of forward progress the peer has made on incoming streams
    pub fn incoming_bytes_progressed(&self) -> VarInt {
        self.inner
            .incoming_connection_flow_controller
            .acquired_window()
    }

    /// The number of bytes of forward progress the local endpoint has made on outgoing streams
    pub fn outgoing_bytes_progressed(&self) -> VarInt {
        self.inner
            .outgoing_connection_flow_controller
            .acquired_window()
    }

    /// Accepts the next incoming stream of a given type
    pub fn poll_accept(
        &mut self,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<StreamId>, connection::Error>> {
        macro_rules! with_stream_type {
            (| $stream_type:ident | $block:stmt) => {
                if stream_type == None || stream_type == Some(StreamType::Bidirectional) {
                    let $stream_type = StreamType::Bidirectional;
                    $block
                }
                if stream_type == None || stream_type == Some(StreamType::Unidirectional) {
                    let $stream_type = StreamType::Unidirectional;
                    $block
                }
            };
        }

        // Clear a stored Waker
        with_stream_type!(|stream_type| *self.inner.accept_state.waker_mut(stream_type) = None);

        // If the connection was closed we still allow the application to accept
        // Streams which are already known to the StreamManager.
        // This is done for 2 reasons:
        // 1. If the application doesn't interact with the Streams and observes
        //    their close status, they won't get removed from StreamManager due
        //    to missing finalization interest
        // 2. The streams might already have received all data from the peer at
        //    this point, and for applications it can be helpful to act on this
        //    data.

        with_stream_type!(|stream_type| if let Some(stream_id) =
            self.accept_stream_with_type(stream_type)?
        {
            return Ok(Some(stream_id)).into();
        });

        match self.inner.close_reason {
            // The connection closed without an error
            Some(connection::Error::Closed { .. }) => return Ok(None).into(),
            // Translate application closes to end of stream
            Some(connection::Error::Transport { code, .. })
                if code == transport::Error::APPLICATION_ERROR.code =>
            {
                return Ok(None).into()
            }
            // Translate idle timer expiration to end of stream
            Some(connection::Error::IdleTimerExpired { .. }) => return Ok(None).into(),
            Some(reason) => return Err(reason).into(),
            None => {}
        }

        // Store the `Waker` for notifying the application if we accept a Stream
        with_stream_type!(
            |stream_type| *self.inner.accept_state.waker_mut(stream_type) =
                Some(context.waker().clone())
        );

        Poll::Pending
    }

    fn accept_stream_with_type(
        &mut self,
        stream_type: StreamType,
    ) -> Result<Option<StreamId>, connection::Error> {
        // Check if the Stream exists
        let next_id_to_accept = self
            .inner
            .accept_state
            .next_stream_id(stream_type)
            .ok_or_else(connection::Error::stream_id_exhausted)?;

        if self.inner.streams.contains(next_id_to_accept) {
            *self.inner.accept_state.next_stream_mut(stream_type) =
                next_id_to_accept.next_of_type();
            Ok(Some(next_id_to_accept))
        } else {
            Ok(None)
        }
    }

    /// Opens the next local initiated stream of a certain type
    pub fn poll_open_local_stream(
        &mut self,
        stream_type: StreamType,
        open_token: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<Result<StreamId, connection::Error>> {
        // If StreamManager was closed, return the error
        if let Some(error) = self.inner.close_reason {
            return Err(error).into();
        }

        let first_unopened_id =
            ready!(self
                .inner
                .poll_open_local_stream(stream_type, open_token, context))?;

        // Increase the next utilized Stream ID
        *self
            .inner
            .next_stream_ids
            .get_mut(self.inner.local_endpoint_type, stream_type) =
            first_unopened_id.next_of_type();

        Ok(first_unopened_id).into()
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.inner
            .incoming_connection_flow_controller
            .on_packet_ack(ack_set);
        self.inner
            .outgoing_connection_flow_controller
            .on_packet_ack(ack_set);
        self.inner.stream_controller.on_packet_ack(ack_set);

        self.inner.streams.iterate_frame_delivery_list(
            &mut self.inner.stream_controller,
            |stream| {
                // We have to wake inside the lock, since `StreamEvent`s has no capacity
                // to carry wakers in another iteration
                let mut events = StreamEvents::new();
                stream.on_packet_ack(ack_set, &mut events);
                events.wake_all();
            },
        );
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.inner
            .incoming_connection_flow_controller
            .on_packet_loss(ack_set);
        self.inner
            .outgoing_connection_flow_controller
            .on_packet_loss(ack_set);
        self.inner.stream_controller.on_packet_loss(ack_set);

        self.inner.streams.iterate_frame_delivery_list(
            &mut self.inner.stream_controller,
            |stream| {
                // We have to wake inside the lock, since `StreamEvent`s has no capacity
                // to carry wakers in another iteration
                let mut events = StreamEvents::new();
                stream.on_packet_loss(ack_set, &mut events);
                events.wake_all();
            },
        );
    }

    /// This method gets called when the RTT estimate is updated for the active path
    pub fn on_rtt_update(&mut self, rtt_estimator: &RttEstimator) {
        let blocked_sync_period = self.blocked_sync_period(rtt_estimator);

        {
            let last_blocked_sync_period = self.last_blocked_sync_period.as_millis() as u64;
            let current_blocked_sync_period = blocked_sync_period.as_millis() as u64;

            /// The number of milliseconds to which the change comparison is configured
            ///
            /// Ideally this number is a power of 2 so the computation is efficient
            const SENSITIVITY_MS: u64 = 16;

            // If we haven't changed a significant amount, there's no point in updating everything
            if last_blocked_sync_period / SENSITIVITY_MS
                == current_blocked_sync_period / SENSITIVITY_MS
            {
                return;
            }
        }

        self.last_blocked_sync_period = blocked_sync_period;

        self.inner
            .stream_controller
            .update_blocked_sync_period(blocked_sync_period);
        self.inner
            .outgoing_connection_flow_controller
            .update_blocked_sync_period(blocked_sync_period);
        self.inner.streams.iterate_stream_flow_credits_list(
            &mut self.inner.stream_controller,
            |stream| {
                stream.update_blocked_sync_period(blocked_sync_period);
                StreamContainerIterationResult::Continue
            },
        );
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.inner.stream_controller.on_timeout(now);
        self.inner
            .outgoing_connection_flow_controller
            .on_timeout(now);
        self.inner.streams.iterate_stream_flow_credits_list(
            &mut self.inner.stream_controller,
            |stream| {
                stream.on_timeout(now);
                StreamContainerIterationResult::Continue
            },
        );
    }

    /// Closes the [`AbstractStreamManager`] and resets all streams with the
    /// given error. The current implementation will still
    /// allow to forward frames to the contained Streams as well as to query them
    /// for data. However new Streams can not be created.
    pub fn close(&mut self, error: connection::Error) {
        self.inner.close(error, false);
    }

    /// If the `StreamManager` is closed, this returns the error which which was
    /// used to close it.
    pub fn close_reason(&self) -> Option<connection::Error> {
        self.inner.close_reason
    }

    /// Closes the [`AbstractStreamManager`], flushes all send streams and resets all receive streams.
    ///
    /// This is used for when the application drops the connection but still has pending data to
    /// transmit.
    pub fn flush(&mut self, error: connection::Error) -> Poll<()> {
        self.inner.flush(error)
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        self.inner
            .incoming_connection_flow_controller
            .on_transmit(context)?;
        self.inner
            .outgoing_connection_flow_controller
            .on_transmit(context)?;
        self.inner.stream_controller.on_transmit(context)?;

        // Due to an error we could not transmit all data.
        // We add streams which could not send data back into the
        // waiting_for_transmission list, so that they will be queried again
        // the next time transmission capacity is available.
        // We actually add those Streams to the end of the list,
        // since the earlier entries are from Streams which were not
        // able to write all the desired data and added themselves as
        // transmit interested again
        let mut transmit_result = Ok(());

        if context.transmission_constraint().can_retransmit() {
            // ensure components only retransmit in this phase
            let mut retransmission_context =
                transmission::context::RetransmissionContext::new(context);

            // Prioritize retransmitting lost data
            self.inner.streams.iterate_retransmission_list(
                &mut self.inner.stream_controller,
                |stream: &mut S| {
                    transmit_result = stream.on_transmit(&mut retransmission_context);
                    if transmit_result.is_err() {
                        StreamContainerIterationResult::BreakAndInsertAtBack
                    } else {
                        StreamContainerIterationResult::Continue
                    }
                },
            );

            // return if there were any errors
            transmit_result?;
        }

        if context.transmission_constraint().can_transmit() {
            self.inner.streams.iterate_transmission_list(
                &mut self.inner.stream_controller,
                |stream: &mut S| {
                    transmit_result = stream.on_transmit(context);
                    if transmit_result.is_err() {
                        StreamContainerIterationResult::BreakAndInsertAtBack
                    } else {
                        StreamContainerIterationResult::Continue
                    }
                },
            );
        }

        // There is no `finalize_done_streams` here, since we do not expect to
        // perform an operation which brings us in a finalization state

        transmit_result
    }

    /// Calculates the period for sending STREAMS_BLOCKED, STREAM_DATA_BLOCKED and
    /// DATA_BLOCKED frames when blocked, according to the idle timeout and latest RTT estimates
    fn blocked_sync_period(&self, rtt_estimator: &RttEstimator) -> Duration {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //# To keep the
        //# connection from closing, a sender that is flow control limited SHOULD
        //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
        //# has no ack-eliciting packets in flight.

        // STREAMS_BLOCKED, DATA_BLOCKED, and STREAM_DATA_BLOCKED frames are
        // sent to prevent the connection from closing due to an idle timeout
        // when we are blocked from opening or sending on streams. We use a pto count
        // of 1 so the periodic components can track backoff independently.

        // For extremely low RTT networks, this will ensure we do not send blocked
        // frames too frequently.
        const MIN_BLOCKED_SYNC_PERIOD: Duration = Duration::from_millis(5);

        let pto = rtt_estimator.pto_period(1, PacketNumberSpace::ApplicationData);

        pto.max(MIN_BLOCKED_SYNC_PERIOD)
    }

    // Frame reception
    // These functions are called from the packet delivery thread

    /// This method encapsulates all common actions for handling incoming frames
    /// which target a specific `Stream`.
    /// It will open unopened Streams, lookup the required `Stream`,
    /// and then call the provided function on the Stream.
    /// If this leads to a connection error it will reset all internal connections.
    fn handle_stream_frame<F>(
        &mut self,
        stream_id: StreamId,
        mut func: F,
    ) -> Result<(), transport::Error>
    where
        F: FnMut(&mut S, &mut StreamEvents) -> Result<(), transport::Error>,
    {
        let mut events = StreamEvents::new();

        let result = {
            // If Stream handling causes an error, trigger an internal reset
            self.inner.reset_streams_on_error(|state| {
                // Open streams if necessary
                state.open_stream_if_necessary(stream_id)?;
                // Apply the provided function on the Stream.
                // If the Stream does not exist it is no error.
                state
                    .streams
                    .with_stream(stream_id, &mut state.stream_controller, |stream| {
                        func(stream, &mut events)
                    })
                    .unwrap_or(Ok(()))
            })
        };

        // We wake `Waker`s outside of the Mutex to reduce contention.
        // TODO: This is now no longer outside the Mutex
        events.wake_all();
        result
    }

    /// This is called when a `STREAM_DATA` frame had been received for
    /// a stream
    pub fn on_data(&mut self, frame: &StreamRef) -> Result<(), transport::Error> {
        let stream_id = StreamId::from_varint(frame.stream_id);
        self.handle_stream_frame(stream_id, |stream, events| stream.on_data(frame, events))
    }

    /// This is called when a `DATA_BLOCKED` frame had been received
    pub fn on_data_blocked(&mut self, _frame: DataBlocked) -> Result<(), transport::Error> {
        Ok(()) // This is currently ignored
    }

    /// This is called when a `STREAM_DATA_BLOCKED` frame had been received for
    /// a stream
    pub fn on_stream_data_blocked(
        &mut self,
        frame: &StreamDataBlocked,
    ) -> Result<(), transport::Error> {
        let stream_id = StreamId::from_varint(frame.stream_id);
        self.handle_stream_frame(stream_id, |stream, events| {
            stream.on_stream_data_blocked(frame, events)
        })
    }

    /// This is called when a `RESET_STREAM` frame had been received for
    /// a stream
    pub fn on_reset_stream(&mut self, frame: &ResetStream) -> Result<(), transport::Error> {
        let stream_id = StreamId::from_varint(frame.stream_id);
        self.handle_stream_frame(stream_id, |stream, events| stream.on_reset(frame, events))
    }

    /// This is called when a `MAX_STREAM_DATA` frame had been received for
    /// a stream
    pub fn on_max_stream_data(&mut self, frame: &MaxStreamData) -> Result<(), transport::Error> {
        let stream_id = StreamId::from_varint(frame.stream_id);
        self.handle_stream_frame(stream_id, |stream, events| {
            stream.on_max_stream_data(frame, events)
        })
    }

    /// This is called when a `STOP_SENDING` frame had been received for
    /// a stream
    pub fn on_stop_sending(&mut self, frame: &StopSending) -> Result<(), transport::Error> {
        let stream_id = StreamId::from_varint(frame.stream_id);
        self.handle_stream_frame(stream_id, |stream, events| {
            stream.on_stop_sending(frame, events)
        })
    }

    /// This is called when a `MAX_DATA` frame had been received
    pub fn on_max_data(&mut self, frame: MaxData) -> Result<(), transport::Error> {
        self.inner
            .outgoing_connection_flow_controller
            .on_max_data(frame);

        if self
            .inner
            .outgoing_connection_flow_controller
            .available_window()
            == VarInt::from_u32(0)
        {
            return Ok(());
        }

        // Iterate over streams and allow them to grab credits from the
        // connection window. As soon as we run out of credits we stop
        // iterating and insert the remaining streams to the end of the list
        // again.
        let conn_flow = &mut self.inner.outgoing_connection_flow_controller;
        self.inner.streams.iterate_connection_flow_credits_list(
            &mut self.inner.stream_controller,
            |stream| {
                stream.on_connection_window_available();

                if conn_flow.available_window() == VarInt::from_u32(0) {
                    StreamContainerIterationResult::BreakAndInsertAtBack
                } else {
                    StreamContainerIterationResult::Continue
                }
            },
        );

        Ok(())
    }

    /// This is called when a `STREAMS_BLOCKED` frame had been received
    pub fn on_streams_blocked(&mut self, _frame: &StreamsBlocked) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
        //= type=TODO
        //= tracking-issue=244
        //= feature=Stream concurrency
        Ok(()) // TODO: Implement me
    }

    /// This is called when a `MAX_STREAMS` frame had been received
    pub fn on_max_streams(&mut self, frame: &MaxStreams) -> Result<(), transport::Error> {
        self.inner.stream_controller.on_max_streams(frame);

        Ok(())
    }

    // User APIs

    /// Executes an application API call on the given Stream if the Stream exists
    /// and returns the result of the API call.
    ///
    /// If the Stream does not exist `unknown_stream_result` will be returned.
    ///
    /// If the application call requires transmission of data, the QUIC connection
    /// thread will be notified through the [`WakeHandle`] in the provided [`ConnectionApiCallContext`].
    fn perform_api_call<F, R>(
        &mut self,
        stream_id: StreamId,
        unknown_stream_result: R,
        api_call_context: &mut ConnectionApiCallContext,
        func: F,
    ) -> R
    where
        F: FnOnce(&mut S) -> R,
    {
        let had_transmission_interest = self.inner.streams.has_transmission_interest();

        let result = self
            .inner
            .streams
            .with_stream(stream_id, &mut self.inner.stream_controller, |stream| {
                func(stream)
            })
            .unwrap_or(unknown_stream_result);

        // A wakeup is only triggered if the the transmission list is
        // now empty, but was previously not. The edge triggered behavior
        // minimizes the amount of necessary wakeups.
        let require_wakeup =
            !had_transmission_interest && self.inner.streams.has_transmission_interest();

        // TODO: This currently wakes the connection task while inside the connection Mutex.
        // It will be better if we return the `Waker` instead and perform the wakeup afterwards.
        if require_wakeup {
            api_call_context.wakeup_handle().wakeup();
        }

        result
    }

    pub fn poll_request(
        &mut self,
        stream_id: StreamId,
        api_call_context: &mut ConnectionApiCallContext,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        self.perform_api_call(
            stream_id,
            Err(StreamError::invalid_stream()),
            api_call_context,
            |stream| stream.poll_request(request, context),
        )
    }

    /// Returns whether or not streams have data to send
    pub fn has_pending_streams(&self) -> bool {
        self.inner.streams.has_pending_streams()
    }
}

impl<S: StreamTrait> timer::Provider for AbstractStreamManager<S> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.inner.stream_controller.timers(query)?;
        self.inner
            .outgoing_connection_flow_controller
            .timers(query)?;
        self.inner.streams.timers(query)?;
        Ok(())
    }
}

impl<S: StreamTrait> transmission::interest::Provider for AbstractStreamManager<S> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.inner.streams.transmission_interest(query)?;
        self.inner.stream_controller.transmission_interest(query)?;
        self.inner
            .incoming_connection_flow_controller
            .transmission_interest(query)?;
        self.inner
            .outgoing_connection_flow_controller
            .transmission_interest(query)?;

        Ok(())
    }
}

impl<S: StreamTrait> connection::finalization::Provider for AbstractStreamManager<S> {
    fn finalization_status(&self) -> connection::finalization::Status {
        if self.inner.close_reason.is_some() && self.inner.streams.nr_active_streams() == 0 {
            connection::finalization::Status::Final
        } else if self.inner.close_reason.is_some() && self.inner.streams.nr_active_streams() > 0 {
            connection::finalization::Status::Draining
        } else {
            connection::finalization::Status::Idle
        }
    }
}

// These are methods that StreamManager only exposes for test purposes.
//
// They might perform additional allocations, and may not be as safe to call
// due to being allowed to panic! when invariants are violated.

#[cfg(test)]
impl<S: StreamTrait> AbstractStreamManager<S> {
    /// Executes the given function using the outgoing flow controller
    pub fn with_outgoing_connection_flow_controller<F, R>(&mut self, func: F) -> R
    where
        F: FnOnce(&mut OutgoingConnectionFlowController) -> R,
    {
        func(&mut self.inner.outgoing_connection_flow_controller)
    }

    /// Executes the given function using the stream controller
    pub fn with_stream_controller<F, R>(&mut self, func: F) -> R
    where
        F: FnOnce(&mut stream::Controller) -> R,
    {
        func(&mut self.inner.stream_controller)
    }

    /// Asserts that a Stream with the given ID exists, and executes the provided
    /// function on it
    pub fn with_asserted_stream<F, R>(&mut self, stream_id: StreamId, func: F) -> R
    where
        F: FnOnce(&mut S) -> R,
    {
        self.inner
            .streams
            .with_stream(stream_id, &mut self.inner.stream_controller, func)
            .expect("Stream is open")
    }

    /// Returns the list of Stream IDs which is currently tracked by the
    /// [`StreamManager`].
    pub fn active_streams(&mut self) -> Vec<StreamId> {
        let mut results = Vec::new();
        self.inner
            .streams
            .iterate_streams(&mut self.inner.stream_controller, |stream| {
                results.push(stream.stream_id())
            });
        results
    }

    /// Returns the list of Stream IDs for Streams which are waiting for
    /// connection flow control credits.
    pub fn streams_waiting_for_connection_flow_control_credits(&mut self) -> Vec<StreamId> {
        let mut results = Vec::new();
        self.inner.streams.iterate_connection_flow_credits_list(
            &mut self.inner.stream_controller,
            |stream| {
                results.push(stream.stream_id());
                StreamContainerIterationResult::Continue
            },
        );
        results
    }

    /// Returns the list of Stream IDs for Streams which are waiting for
    /// delivery notifications.
    pub fn streams_waiting_for_delivery_notifications(&mut self) -> Vec<StreamId> {
        let mut results = Vec::new();
        self.inner.streams.iterate_frame_delivery_list(
            &mut self.inner.stream_controller,
            |stream| {
                results.push(stream.stream_id());
            },
        );
        results
    }

    /// Returns the list of Stream IDs for Streams which are waiting for
    /// transmission.
    pub fn streams_waiting_for_transmission(&mut self) -> Vec<StreamId> {
        let mut results = Vec::new();
        self.inner
            .streams
            .iterate_transmission_list(&mut self.inner.stream_controller, |stream| {
                results.push(stream.stream_id());
                StreamContainerIterationResult::Continue
            });
        results
    }

    /// Returns the list of Stream IDs for Streams which are waiting for
    /// retransmission.
    pub fn streams_waiting_for_retransmission(&mut self) -> Vec<StreamId> {
        let mut results = Vec::new();
        self.inner.streams.iterate_retransmission_list(
            &mut self.inner.stream_controller,
            |stream| {
                results.push(stream.stream_id());
                StreamContainerIterationResult::Continue
            },
        );
        results
    }
}

#[cfg(test)]
mod tests;
