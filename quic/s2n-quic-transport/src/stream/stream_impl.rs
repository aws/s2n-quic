// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    stream::{
        incoming_connection_flow_controller::IncomingConnectionFlowController,
        outgoing_connection_flow_controller::OutgoingConnectionFlowController,
        receive_stream::ReceiveStream,
        send_stream::SendStream,
        stream_events::StreamEvents,
        stream_interests::{StreamInterestProvider, StreamInterests},
        StreamError,
    },
};
use core::{task::Context, time::Duration};
use s2n_quic_core::{
    ack, endpoint,
    frame::{stream::StreamRef, MaxStreamData, ResetStream, StopSending, StreamDataBlocked},
    stream::{ops, StreamId},
    time::{timer, Timestamp},
    transport,
    varint::VarInt,
};

/// Configuration values for a Stream
#[derive(Debug)]
pub struct StreamConfig {
    /// The [`Stream`]s identifier
    pub stream_id: StreamId,
    /// The type of the local endpoint
    pub local_endpoint_type: endpoint::Type,
    /// The connection-wide flow controller for receiving data
    pub incoming_connection_flow_controller: IncomingConnectionFlowController,
    /// The connection-wide flow controller for sending data
    pub outgoing_connection_flow_controller: OutgoingConnectionFlowController,
    /// The initial flow control window for receiving data
    pub initial_receive_window: VarInt,
    /// The desired flow control window that we want to maintain on the receiving side
    pub desired_flow_control_window: u32,
    /// The initial flow control window for sending data
    pub initial_send_window: VarInt,
    /// The maximum buffered amount of data on the sending side
    pub max_send_buffer_size: u32,
}

/// A trait which represents an internally used `Stream`
pub trait StreamTrait: StreamInterestProvider + timer::Provider {
    /// Creates a new `Stream` instance with the given configuration
    fn new(config: StreamConfig) -> Self;

    /// Returns the Streams ID
    fn stream_id(&self) -> StreamId;

    // These functions are called from the packet delivery thread

    /// This is called when a `STREAM_DATA` frame had been received for
    /// this stream
    fn on_data(
        &mut self,
        frame: &StreamRef,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error>;

    /// This is called when a `STREAM_DATA_BLOCKED` frame had been received for
    /// this stream
    fn on_stream_data_blocked(
        &mut self,
        frame: &StreamDataBlocked,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error>;

    /// This is called when a `RESET_STREAM` frame had been received for
    /// this stream
    fn on_reset(
        &mut self,
        frame: &ResetStream,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error>;

    /// This is called when a `MAX_STREAM_DATA` frame had been received for
    /// this stream
    fn on_max_stream_data(
        &mut self,
        frame: &MaxStreamData,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error>;

    /// This is called when a `STOP_SENDING` frame had been received for
    /// this stream
    fn on_stop_sending(
        &mut self,
        frame: &StopSending,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error>;

    /// This method gets called when a packet delivery got acknowledged
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A, events: &mut StreamEvents);

    /// This method gets called when a packet loss is reported
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A, events: &mut StreamEvents);

    /// Updates the period at which `STREAM_DATA_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration);

    /// Called when the connection timer expires
    fn on_timeout(&mut self, now: Timestamp);

    /// This method gets called when a stream gets reset due to a reason that is
    /// not related to a frame. E.g. due to a connection failure.
    fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents);

    /// Queries the component for any outgoing frames that need to get sent
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError>;

    /// This method is called when a connection window is available
    fn on_connection_window_available(&mut self);

    // These functions are called from the client API

    fn poll_request(
        &mut self,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError>;
}

/// The implementation of a `Stream`.
/// This is mostly a facade over the reading and writing half of the `Stream`.
pub struct StreamImpl {
    /// The stream ID
    pub(super) stream_id: StreamId,
    /// Manages the receiving side of the stream
    pub(super) receive_stream: ReceiveStream,
    /// Manages the sending side of the stream
    pub(super) send_stream: SendStream,
}

impl StreamImpl {
    fn poll_request_impl(
        &mut self,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        let mut response = ops::Response::default();
        if let Some(rx) = request.rx.as_mut() {
            response.rx = Some(self.receive_stream.poll_request(rx, context)?);
        }
        if let Some(tx) = request.tx.as_mut() {
            response.tx = Some(self.send_stream.poll_request(tx, context)?);
        }

        Ok(response)
    }
}

impl StreamTrait for StreamImpl {
    fn new(config: StreamConfig) -> StreamImpl {
        let receive_is_closed = config.stream_id.stream_type().is_unidirectional()
            && config.stream_id.initiator() == config.local_endpoint_type;
        let send_is_closed = config.stream_id.stream_type().is_unidirectional()
            && config.stream_id.initiator() != config.local_endpoint_type;

        StreamImpl {
            stream_id: config.stream_id,
            receive_stream: ReceiveStream::new(
                receive_is_closed,
                config.incoming_connection_flow_controller,
                config.initial_receive_window,
                config.desired_flow_control_window,
            ),
            send_stream: SendStream::new(
                config.outgoing_connection_flow_controller,
                send_is_closed,
                config.initial_send_window,
                config.max_send_buffer_size,
            ),
        }
    }

    #[inline]
    fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    // These functions are called from the packet delivery thread

    #[inline]
    fn on_data(
        &mut self,
        frame: &StreamRef,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        self.receive_stream.on_data(frame, events)
    }

    #[inline]
    fn on_stream_data_blocked(
        &mut self,
        frame: &StreamDataBlocked,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        self.receive_stream.on_stream_data_blocked(frame, events)
    }

    #[inline]
    fn on_reset(
        &mut self,
        frame: &ResetStream,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        self.receive_stream.on_reset(frame, events)
    }

    #[inline]
    fn on_max_stream_data(
        &mut self,
        frame: &MaxStreamData,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        self.send_stream.on_max_stream_data(frame, events)
    }

    #[inline]
    fn on_stop_sending(
        &mut self,
        frame: &StopSending,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        self.send_stream.on_stop_sending(frame, events)
    }

    #[inline]
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A, events: &mut StreamEvents) {
        self.receive_stream.on_packet_ack(ack_set);
        self.send_stream.on_packet_ack(ack_set, events);
    }

    #[inline]
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A, _events: &mut StreamEvents) {
        self.receive_stream.on_packet_loss(ack_set);
        self.send_stream.on_packet_loss(ack_set);
    }

    #[inline]
    fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.send_stream
            .update_blocked_sync_period(blocked_sync_period);
    }

    #[inline]
    fn on_timeout(&mut self, now: Timestamp) {
        self.send_stream.on_timeout(now)
    }

    #[inline]
    fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents) {
        self.receive_stream.on_internal_reset(error, events);
        self.send_stream.on_internal_reset(error, events);
    }

    #[inline]
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Query the receiving side for outgoing data
        self.receive_stream.on_transmit(self.stream_id, context)?;
        // And the sending side
        self.send_stream.on_transmit(self.stream_id, context)
    }

    #[inline]
    fn on_connection_window_available(&mut self) {
        self.send_stream.on_connection_window_available()
    }

    // These functions are called from the client API

    fn poll_request(
        &mut self,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError> {
        #[cfg(debug_assertions)]
        let contract: crate::stream::contract::Request = (&*request).into();

        let result = self.poll_request_impl(request, context);

        #[cfg(debug_assertions)]
        contract.validate_response(request, result.as_ref(), context);

        result
    }
}

impl timer::Provider for StreamImpl {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.send_stream.timers(query)?;
        Ok(())
    }
}

impl StreamInterestProvider for StreamImpl {
    #[inline]
    fn stream_interests(&self, interests: &mut StreamInterests) {
        self.send_stream.stream_interests(interests);
        self.receive_stream.stream_interests(interests);
    }
}
