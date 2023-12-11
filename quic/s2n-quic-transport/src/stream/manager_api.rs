// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    contexts::{ConnectionApiCallContext, OnTransmitError, WriteContext},
    recovery::RttEstimator,
    stream::StreamError,
    transmission,
};
use core::{
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::{
    ack, endpoint,
    frame::{
        stream::StreamRef, DataBlocked, MaxData, MaxStreamData, MaxStreams, ResetStream,
        StopSending, StreamDataBlocked, StreamsBlocked,
    },
    stream::{ops, StreamId, StreamType},
    time::{timer, Timestamp},
    transport::{self, parameters::InitialFlowControlLimits},
    varint::VarInt,
};

pub trait Manager:
    'static
    + Send
    + timer::Provider
    + transmission::interest::Provider
    + connection::finalization::Provider
    + core::fmt::Debug
{
    /// Creates a new stream manager using the provided configuration parameters
    fn new(
        connection_limits: &connection::Limits,
        local_endpoint_type: endpoint::Type,
        initial_local_limits: InitialFlowControlLimits,
        initial_peer_limits: InitialFlowControlLimits,
        min_rtt: Duration,
    ) -> Self;

    /// The number of bytes of forward progress the peer has made on incoming streams
    fn incoming_bytes_progressed(&self) -> VarInt;

    /// The number of bytes of forward progress the local endpoint has made on outgoing streams
    fn outgoing_bytes_progressed(&self) -> VarInt;

    /// Accepts the next incoming stream of a given type
    fn poll_accept(
        &mut self,
        stream_type: Option<StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<StreamId>, connection::Error>>;

    /// Opens the next local initiated stream of a certain type
    fn poll_open_local_stream(
        &mut self,
        stream_type: StreamType,
        open_token: &mut connection::OpenToken,
        api_call_context: &mut ConnectionApiCallContext,
        context: &Context,
    ) -> Poll<Result<StreamId, connection::Error>>;

    /// This method gets called when a packet delivery got acknowledged
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A);

    /// This method gets called when a packet loss is reported
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A);

    /// This method gets called when the RTT estimate is updated for the active path
    fn on_rtt_update(&mut self, rtt_estimator: &RttEstimator, now: Timestamp);

    /// Called when the connection timer expires
    fn on_timeout(&mut self, now: Timestamp);

    /// Closes the manager and resets all streams with the
    /// given error. The current implementation will still
    /// allow to forward frames to the contained Streams as well as to query them
    /// for data. However new Streams can not be created.
    fn close(&mut self, error: connection::Error);

    /// If the manager is closed, this returns the error which which was
    /// used to close it.
    fn close_reason(&self) -> Option<connection::Error>;

    /// Closes the manager, flushes all send streams and resets all receive streams.
    ///
    /// This is used for when the application drops the connection but still has pending data to
    /// transmit.
    fn flush(&mut self, error: connection::Error) -> Poll<()>;

    /// Queries the component for any outgoing frames that need to get sent
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError>;

    // Frame reception
    // These functions are called from the packet delivery thread

    /// This is called when a `STREAM_DATA` frame had been received for
    /// a stream
    fn on_data(&mut self, frame: &StreamRef) -> Result<(), transport::Error>;

    /// This is called when a `DATA_BLOCKED` frame had been received
    fn on_data_blocked(&mut self, frame: DataBlocked) -> Result<(), transport::Error>;

    /// This is called when a `STREAM_DATA_BLOCKED` frame had been received for
    /// a stream
    fn on_stream_data_blocked(&mut self, frame: &StreamDataBlocked)
        -> Result<(), transport::Error>;

    /// This is called when a `RESET_STREAM` frame had been received for
    /// a stream
    fn on_reset_stream(&mut self, frame: &ResetStream) -> Result<(), transport::Error>;

    /// This is called when a `MAX_STREAM_DATA` frame had been received for
    /// a stream
    fn on_max_stream_data(&mut self, frame: &MaxStreamData) -> Result<(), transport::Error>;

    /// This is called when a `STOP_SENDING` frame had been received for
    /// a stream
    fn on_stop_sending(&mut self, frame: &StopSending) -> Result<(), transport::Error>;

    /// This is called when a `MAX_DATA` frame had been received
    fn on_max_data(&mut self, frame: MaxData) -> Result<(), transport::Error>;

    /// This is called when a `STREAMS_BLOCKED` frame had been received
    fn on_streams_blocked(&mut self, frame: &StreamsBlocked) -> Result<(), transport::Error>;

    /// This is called when a `MAX_STREAMS` frame had been received
    fn on_max_streams(&mut self, frame: &MaxStreams) -> Result<(), transport::Error>;

    // User APIs

    fn poll_request(
        &mut self,
        stream_id: StreamId,
        api_call_context: &mut ConnectionApiCallContext,
        request: &mut ops::Request,
        context: Option<&Context>,
    ) -> Result<ops::Response, StreamError>;

    /// Returns whether or not streams have data to send
    fn has_pending_streams(&self) -> bool;
}
