// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    stream::{
        outgoing_connection_flow_controller::OutgoingConnectionFlowController,
        stream_events::StreamEvents,
        stream_interests::{StreamInterestProvider, StreamInterests},
        StreamError,
    },
    sync::{
        data_sender::{self, DataSender, OutgoingDataFlowController},
        OnceSync, PeriodicSync, ValueToFrameWriter,
    },
    transmission,
    transmission::interest::Provider as _,
};
use bytes::Bytes;
use core::{
    convert::TryFrom,
    task::{Context, Waker},
    time::Duration,
};
use s2n_quic_core::{
    ack, application,
    frame::{MaxStreamData, ResetStream, StopSending, StreamDataBlocked},
    packet::number::PacketNumber,
    stream::{ops, StreamId},
    time::{timer, Timestamp},
    transport,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-3.1
//#          o
//#          | Create Stream (Sending)
//#          | Peer Creates Bidirectional Stream
//#          v
//#      +-------+
//#      | Ready | Send RESET_STREAM
//#      |       |-----------------------.
//#      +-------+                       |
//#          |                           |
//#          | Send STREAM /             |
//#          |      STREAM_DATA_BLOCKED  |
//#          v                           |
//#      +-------+                       |
//#      | Send  | Send RESET_STREAM     |
//#      |       |---------------------->|
//#      +-------+                       |
//#          |                           |
//#          | Send STREAM + FIN         |
//#          v                           v
//#      +-------+                   +-------+
//#      | Data  | Send RESET_STREAM | Reset |
//#      | Sent  |------------------>| Sent  |
//#      +-------+                   +-------+
//#          |                           |
//#          | Recv All ACKs             | Recv ACK
//#          v                           v
//#      +-------+                   +-------+
//#      | Data  |                   | Reset |
//#      | Recvd |                   | Recvd |
//#      +-------+                   +-------+

/// Enumerates the possible states of the sending side of a stream.
/// These states are equivalent to the ones in the QUIC transport specification.
#[derive(PartialEq, Debug, Copy, Clone)]
pub(super) enum SendStreamState {
    /// The stream is still sending data to the remote peer. This state
    /// covers the `Ready`, `Send`, and `DataSent` states of the QUIC
    /// specification.
    ///
    /// As long as the user is actively sending data, most operations are
    /// delegated to the `DataSender`.
    Sending,
    /// The connection was reset. The reset was not yet acknowledged by the
    /// peer. The flag indicates whether the reset state had already been
    /// observed by the user.
    ResetSent(StreamError),
    /// The connection was reset. The reset was acknowledged by the
    /// peer. The flag indicates whether the reset state had already been
    /// observed by the user.
    /// This equals the `ResetRecvd` state from the QUIC specification. But
    /// since we don't receive but rather acknowledge a RESET, we call it this
    /// way.
    ResetAcknowledged(StreamError),
}

/// Stores all data that needs to get transmitted in outgoing RESET frames
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct OutgoingResetData {
    /// The final size which should get transmitted in the RESET frame
    final_size: VarInt,
    /// The error code which should get transmitted in the RESET frame
    application_error_code: application::Error,
}

/// Writes the `RESET` frames based on the streams flow control window.
#[derive(Debug, Default)]
pub struct ResetStreamToFrameWriter {}

impl ValueToFrameWriter<OutgoingResetData> for ResetStreamToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: OutgoingResetData,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&ResetStream {
            stream_id: stream_id.into(),
            application_error_code: value.application_error_code.into(),
            final_size: value.final_size,
        })
    }
}

/// Identifies the source of a Stream reset.
/// Streams resets can be initiated
/// - locally via QUIC Stream API call
/// - locally through the connection in cause of a connection error
/// - from the remote peer by sending a STOP_SENDING frame.
#[derive(Debug, Copy, Clone, PartialEq)]
enum ResetSource {
    /// The reset had been initiated by the local application calling the
    /// `reset()` method on the Stream
    LocalApplication,
    /// The reset had been initiated from the remote through a STOP_SENDING
    /// frame.
    StopSendingFrame,
    /// The reset had been initiated as an internal reset. Likely caused by a
    /// connection error or termination.
    InternalReset,
}

impl ResetSource {
    /// Returns `true` if the Stream reset had been initiated by the
    /// local application
    fn is_local_application(self) -> bool {
        self == ResetSource::LocalApplication
    }

    /// Returns true if the reset is an internal reset
    fn is_internal(self) -> bool {
        self == ResetSource::InternalReset
    }
}

/// The result of a `init_reset()` call.
#[derive(Debug, Copy, Clone, PartialEq)]
#[must_use]
enum InitResetResult {
    /// Reset had been initiated
    ResetInitiated,
    /// The reset was not necessary
    ResetNotNecessary,
}

/// Enumerates states of the [`StreamFlowController`]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum StreamFlowControllerState {
    /// The flow controller did not block transmission of data the last time
    /// it was queried for a window
    Ready,
    /// The flow controller did block transmission of data due to an insufficient
    /// Stream window
    BlockedOnStreamWindow,
    /// The flow controller did block transmission of data due to an insufficient
    /// Connection window
    BlockedOnConnectionWindow,
    /// `finish()` was called on the `StreamFlowController`. No further
    /// interactions are expected.
    Finished,
}

/// Manages the flow control for sending data.
///
/// Flow control is maintained based on
/// - The Stream flow control window (indicated by `MAX_STREAM_DATA`)
/// - The Connection flow control window (indicated by `MAX_DATA`)
///
/// In order to adhere to the connection flow control window the controller will
/// acquire chunks from the connection flow controller whenever necessary.
#[derive(Debug)]
pub(super) struct StreamFlowController {
    /// The flow control manager for the whole connection
    connection_flow_controller: OutgoingConnectionFlowController,
    /// The flow control window acquired from the connection
    acquired_connection_flow_controller_window: VarInt,
    /// The highest offset which was ever tried to be acquired via
    /// the `acquire_flow_control_window()` method
    highest_requested_connection_flow_control_window: VarInt,
    /// The maximum data offset we are allowed to send, which is communicated
    /// via `MAX_STREAM_DATA` frames.
    max_stream_data: VarInt,
    state: StreamFlowControllerState,
    /// For periodically sending `STREAM_DATA_BLOCKED` frames when blocked by peer limits
    stream_data_blocked_sync: PeriodicSync<VarInt, StreamDataBlockedToFrameWriter>,
}

impl StreamFlowController {
    /// Creates a new `StreamFlowController`
    pub fn new(
        connection_flow_controller: OutgoingConnectionFlowController,
        initial_window: VarInt,
    ) -> Self {
        Self {
            connection_flow_controller,
            acquired_connection_flow_controller_window: VarInt::from_u32(0),
            highest_requested_connection_flow_control_window: VarInt::from_u32(0),
            max_stream_data: initial_window,
            state: StreamFlowControllerState::Ready,
            stream_data_blocked_sync: PeriodicSync::new(),
        }
    }

    /// Updates the `MAXIMUM_STREAM_DATA` value which was communicated by a peer
    pub fn set_max_stream_data(&mut self, max_stream_data: VarInt) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //# A sender MUST ignore any MAX_STREAM_DATA or MAX_DATA frames that do
        //# not increase flow control limits.
        if max_stream_data <= self.max_stream_data {
            return;
        }

        self.max_stream_data = max_stream_data;
        if self.state == StreamFlowControllerState::BlockedOnStreamWindow {
            self.state = StreamFlowControllerState::Ready;
            // We now have more capacity from the peer so stop sending STREAM_DATA_BLOCKED frames
            self.stream_data_blocked_sync.stop_sync();
        }
    }

    /// Tries to acquire as much window from the connection flow control window
    /// as possible.
    pub fn try_acquire_connection_window(&mut self) {
        // We are not allowed to modify the connection window after the
        // stream finished.
        if self.state == StreamFlowControllerState::Finished {
            return;
        }

        let missing_connection_window = self
            .highest_requested_connection_flow_control_window
            .saturating_sub(self.acquired_connection_flow_controller_window);

        if missing_connection_window > VarInt::from_u32(0) {
            // Acquire as much window from the connection as possible to satisfy
            // the full range. We might get any amount of window back from it.
            // This includes getting 0 extra bytes.
            let acquired = self
                .connection_flow_controller
                .acquire_window(missing_connection_window);
            self.acquired_connection_flow_controller_window += acquired;
            if acquired > VarInt::from_u32(0)
                && self.state == StreamFlowControllerState::BlockedOnConnectionWindow
            {
                self.state = StreamFlowControllerState::Ready;
            }
        }
    }

    /// Returns the window/offset up to which data can be written
    fn available_window(&self) -> VarInt {
        core::cmp::min(
            self.max_stream_data,
            self.acquired_connection_flow_controller_window,
        )
    }

    /// Returns the state of the flow controller
    pub fn state(&self) -> StreamFlowControllerState {
        self.state
    }

    /// Returns the total connection window which has been acquired for this
    /// Stream.
    pub fn acquired_connection_flow_controller_window(&self) -> VarInt {
        self.acquired_connection_flow_controller_window
    }

    /// This method is called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.stream_data_blocked_sync.on_packet_ack(ack_set)
    }

    /// This method is called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        // FIXME: Should we still do this?
        self.stream_data_blocked_sync.on_packet_loss(ack_set);
    }

    /// Updates the period at which `STREAM_DATA_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    pub fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.stream_data_blocked_sync
            .update_sync_period(blocked_sync_period);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //# To keep the
        //# connection from closing, a sender that is flow control limited SHOULD
        //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
        //# has no ack-eliciting packets in flight.
        if context.ack_elicitation().is_ack_eliciting()
            && self.stream_data_blocked_sync.has_delivered()
        {
            // We are already sending an ack-eliciting packet, so no need to send another STREAM_DATA_BLOCKED
            self.stream_data_blocked_sync
                .skip_delivery(context.current_time());
            Ok(())
        } else {
            self.stream_data_blocked_sync
                .on_transmit(stream_id, context)
        }
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.stream_data_blocked_sync.on_timeout(now)
    }
}

impl timer::Provider for StreamFlowController {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.stream_data_blocked_sync.timers(query)?;
        Ok(())
    }
}

/// Queries the component for interest in transmitting frames
impl transmission::interest::Provider for StreamFlowController {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.stream_data_blocked_sync.transmission_interest(query)
    }
}

impl OutgoingDataFlowController for StreamFlowController {
    fn acquire_flow_control_window(&mut self, end_offset: VarInt) -> VarInt {
        debug_assert_ne!(
            StreamFlowControllerState::Finished,
            self.state,
            "Trying to acquire window from finished stream"
        );
        if self.state == StreamFlowControllerState::Finished {
            return self.available_window();
        }
        self.state = StreamFlowControllerState::Ready;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //# A sender SHOULD send a
        //# STREAM_DATA_BLOCKED or DATA_BLOCKED frame to indicate to the receiver
        //# that it has data to write but is blocked by flow control limits.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
        //# To keep the
        //# connection from closing, a sender that is flow control limited SHOULD
        //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
        //# has no ack-eliciting packets in flight.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.13
        //# A sender SHOULD send a STREAM_DATA_BLOCKED frame (type=0x15) when it
        //# wishes to send data, but is unable to do so due to stream-level flow
        //# control.

        if end_offset > self.max_stream_data {
            // Can't send any data due to being blocked on the Stream window
            self.state = StreamFlowControllerState::BlockedOnStreamWindow;
            self.stream_data_blocked_sync
                .request_delivery(self.max_stream_data);
        }

        self.highest_requested_connection_flow_control_window = core::cmp::max(
            end_offset,
            self.highest_requested_connection_flow_control_window,
        );
        self.try_acquire_connection_window();

        if end_offset > self.acquired_connection_flow_controller_window {
            // Can't send due to being blocked on the connection flow control window
            self.state = StreamFlowControllerState::BlockedOnConnectionWindow;
        }

        self.available_window()
    }

    fn is_blocked(&self) -> bool {
        match self.state {
            StreamFlowControllerState::BlockedOnConnectionWindow
            | StreamFlowControllerState::BlockedOnStreamWindow => true,
            StreamFlowControllerState::Finished | StreamFlowControllerState::Ready => false,
        }
    }

    fn clear_blocked(&mut self) {
        if self.state != StreamFlowControllerState::Finished {
            self.state = StreamFlowControllerState::Ready;
        }
        self.stream_data_blocked_sync.stop_sync();
    }

    fn finish(&mut self) {
        self.state = StreamFlowControllerState::Finished;
        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.3
        //# A sender MUST NOT send a STREAM or
        //# STREAM_DATA_BLOCKED frame for a stream in the "Reset Sent" state or
        //# any terminal state -- that is, after sending a RESET_STREAM frame.
        self.stream_data_blocked_sync.stop_sync();
    }
}

/// Writes the `STREAM_DATA_BLOCKED` frames.
#[derive(Debug, Default)]
pub(super) struct StreamDataBlockedToFrameWriter {}

impl ValueToFrameWriter<VarInt> for StreamDataBlockedToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&StreamDataBlocked {
            stream_id: stream_id.into(),
            stream_data_limit: value,
        })
    }
}

/// The sending half of a stream
#[derive(Debug)]
pub struct SendStream {
    /// The current state of the stream
    pub(super) state: SendStreamState,
    /// Transmitter for outgoing data
    pub(super) data_sender: DataSender<StreamFlowController, data_sender::writer::Stream>,
    /// Synchronizes sending a `RESET` to the receiver
    pub(super) reset_sync: OnceSync<OutgoingResetData, ResetStreamToFrameWriter>,
    /// The handle of a task that is currently waiting on new incoming data or
    /// on waiting for the finalization process to complete.
    ///
    /// If the second value in the tuple is set to true, the stream should be flushed before waking
    /// the waiter.
    pub(super) write_waiter: Option<(Waker, bool)>,
    /// Whether the final state had already been observed by the application
    final_state_observed: bool,
    /// Marks the stream as detached from the application
    detached: bool,
    /// Marks the stream for reset if packet loss is detected while sending.
    reset_on_loss: bool,
}

impl SendStream {
    pub fn new(
        connection_flow_controller: OutgoingConnectionFlowController,
        is_closed: bool,
        initial_window: VarInt,
        max_buffer_capacity: u32,
    ) -> SendStream {
        // If the stream is created in closed state directly move into the
        // terminal state.
        let state = SendStreamState::Sending;

        let flow_controller = StreamFlowController::new(connection_flow_controller, initial_window);

        let data_sender = if is_closed {
            DataSender::new_finished(flow_controller, max_buffer_capacity)
        } else {
            DataSender::new(flow_controller, max_buffer_capacity)
        };

        let mut result = SendStream {
            state,
            data_sender,
            reset_sync: OnceSync::new(),
            write_waiter: None,
            final_state_observed: is_closed,
            detached: is_closed,
            reset_on_loss: false,
        };

        if is_closed {
            result.reset_sync.stop_sync();
        }

        result
    }

    // These functions are called from the packet delivery thread

    /// This is called when a `MAX_STREAM_DATA` frame had been received for
    /// this stream
    pub fn on_max_stream_data(
        &mut self,
        frame: &MaxStreamData,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        // Window size increments are only important while we are still sending data
        // They **are** still important after the application has already called
        // `finish()` and when we know the final size of the stream.
        // The reason for this is that we allow users to enqueue more data than
        // the maximum flow control window.

        if let SendStreamState::Sending = self.state {
            self.data_sender
                .flow_controller_mut()
                .set_max_stream_data(frame.maximum_stream_data);

            // If the window has been increased we need to unblock waiting writers.
            // However if `finish()` has already been called and the final size
            // of the stream is known, we may not unblock writers, since in this
            // case the writer is waiting for the acknowledgement of the FIN.

            // TODO: With the implemented changes receiving a MAX_STREAM_DATA
            // frame will never lead to a wakeup, because it does not impact
            // the buffer space. It can only allow us to transmit again if we
            // were previously not able to do this.
            // Therefore me might want to remove this.
            if self.data_sender.available_buffer_space() > 0
                && self.data_sender.state() == data_sender::State::Sending
            {
                self.wake(events);
            }
        }

        Ok(())
    }

    /// This is called when a `STOP_SENDING` frame had been received for
    /// this stream
    pub fn on_stop_sending(
        &mut self,
        frame: &StopSending,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.5
        //# A STOP_SENDING frame requests that the receiving endpoint send a
        //# RESET_STREAM frame.  An endpoint that receives a STOP_SENDING frame
        //# MUST send a RESET_STREAM frame if the stream is in the "Ready" or
        //# "Send" state.  If the stream is in the "Data Sent" state, the
        //# endpoint MAY defer sending the RESET_STREAM frame until the packets
        //# containing outstanding data are acknowledged or declared lost.  If
        //# any outstanding data is declared lost, the endpoint SHOULD send a
        //# RESET_STREAM frame instead of retransmitting the data.

        // We do not track whether we transmit a range for the first time or
        // whether it is a retransmit. Therefore we can not decide whether the
        // outstanding data is lost, or has never been transmitted.
        // Assuming the client really does not want any data anymore, we will
        // still simply emit a RESET in all cases.

        // Under these assumptions the reset will happen in exactly the same
        // cases as if the user of the `Stream` would have triggered a reset,
        // and we can delegate to the same method.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.5
        //# An endpoint SHOULD copy the error code from the STOP_SENDING frame to
        //# the RESET_STREAM frame it sends, but it can use any application error
        //# code.
        let error = StreamError::stream_reset(frame.application_error_code.into());

        if self.init_reset(ResetSource::StopSendingFrame, error) == InitResetResult::ResetInitiated
        {
            // Return the waker to wake up potential users of the stream.
            // If the Stream got reset, then blocked writers need to get woken up.
            self.wake(events);
        }

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A, events: &mut StreamEvents) {
        self.data_sender.on_packet_ack(ack_set);
        self.data_sender
            .flow_controller_mut()
            .on_packet_ack(ack_set);

        let should_flush = self.write_waiter.as_ref().map_or(false, |w| w.1);
        let mut should_wake = false;
        self.data_sender
            .flow_controller_mut()
            .on_packet_ack(ack_set);

        match self.state {
            SendStreamState::Sending => {
                match self.data_sender.state() {
                    data_sender::State::Sending if should_flush => {
                        // In this state, the application wanted to ensure the peer has received
                        // all of the data in the stream before continuing (flushed the stream).
                        should_wake = self.data_sender.is_empty() && self.can_push();
                    }
                    data_sender::State::Sending => {
                        // In this state we have to wake up the user if the they can
                        // queue more data for transmission. This is possible if
                        // acknowledging packets removed them from the send queue and
                        // we got additional space in the send queue.
                        should_wake = self.can_push();
                    }
                    data_sender::State::Finishing(f) => {
                        should_wake = self.data_sender.is_empty() && f.is_acknowledged();
                    }
                    data_sender::State::Finished => {
                        // If we have already sent a fin and just waiting for it to be
                        // acknowledged, `on_packet_ack` might have moved us into the
                        // final state due.
                        //
                        // In this state we also wake up the application,
                        // since the finish operation has been confirmed.
                        should_wake = true;
                    }
                    _ => {}
                }
            }
            SendStreamState::ResetSent(error_code) => {
                if self.reset_sync.on_packet_ack(ack_set).is_ready() {
                    // A reset had been acknowledged. Enter the terminal state.
                    self.state = SendStreamState::ResetAcknowledged(error_code);

                    // notify the waiter that the stream is finalized
                    should_wake = true;
                }
            }
            _ => {}
        }

        if should_wake {
            self.wake(events);
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if self.reset_on_loss {
            // FIXME: actual stream error
            //
            // We don't care about whether this actually sent a reset or not, just stopping here.
            let _ = self.init_reset(ResetSource::InternalReset, StreamError::non_writable());
        }

        self.data_sender.on_packet_loss(ack_set);
        self.data_sender
            .flow_controller_mut()
            .on_packet_loss(ack_set);
        self.reset_sync.on_packet_loss(ack_set);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.reset_sync.on_transmit(stream_id, context)?;
        self.data_sender.on_transmit(stream_id.into(), context)?;
        self.data_sender
            .flow_controller_mut()
            .on_transmit(stream_id, context)
    }

    /// Updates the period at which `STREAM_DATA_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    pub fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.data_sender
            .flow_controller_mut()
            .update_blocked_sync_period(blocked_sync_period)
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.data_sender.flow_controller_mut().on_timeout(now)
    }

    /// A reset that is triggered without having received a `RESET` frame.
    pub fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents) {
        let _ = self.init_reset(
            // This is remote in a sense we do not have to emit a message
            ResetSource::InternalReset,
            error,
        );

        // Return the waker to wake up potential users of the stream.
        // If the Stream got reset, then blocked writers need to get woken up.
        self.wake(events);
    }

    pub fn on_flush(&mut self, error: StreamError, events: &mut StreamEvents) {
        match self.data_sender.state() {
            data_sender::State::Finishing(_) => {
                // wait until the data sender is done sending
            }
            _ => {
                // since, we aren't finalizing, any other state should trigger a reset
                self.on_internal_reset(error, events);
            }
        }
    }

    /// This method is called when a connection window is available
    pub fn on_connection_window_available(&mut self) {
        // Outstanding flow control requests are only fulfilled if the Stream
        // was still trying to send data.
        if let SendStreamState::Sending = self.state {
            self.data_sender
                .flow_controller_mut()
                .try_acquire_connection_window();
        }
    }

    /// Wakes up the application on progress updates
    ///
    /// If there is not a registered waker and the stream is in a terminal state,
    /// the stream will be finalized.
    fn wake(&mut self, events: &mut StreamEvents) {
        if let Some((waker, _should_flush)) = self.write_waiter.take() {
            events.store_write_waker(waker);
            return;
        }

        // If the stream is detached from the application, try to make progress
        if self.detached {
            self.detach();
        }
    }

    // These functions are called from the client API

    /// Tries to enqueue data for transmission on the `Stream`.
    ///
    /// The method will succeed as long as buffering space is available for the data,
    /// and as long as the `Stream` is still in the `Sending` state. If `finish()`
    /// had been called before, or if the `Stream` was reset in between, the
    /// method will return a [`StreamError`].
    ///
    /// If the request was not able to consume all provided chunks, the optional context will be notified
    /// when more space is available.
    ///
    /// The first call to [`finish()`] will start the finalization process. In this
    /// case the `FIN` flag for the `Stream` will be transmitted to the peer as
    /// soon as possible. The method will return `Poll::Pending` as long as not
    /// all outgoing data has been acknowledged by the peer. As soon as all data
    /// had been acknowledged the method will return `Poll::Ready(())`.
    ///
    /// If the `Stream` gets reset while waiting for acknowledgement of all
    /// outstanding data the method will return a [`StreamError`].
    pub fn poll_request(
        &mut self,
        request: &mut ops::tx::Request,
        context: Option<&Context>,
    ) -> Result<ops::tx::Response, StreamError> {
        let mut response = ops::tx::Response::default();

        if request.detached {
            self.detach();
        }

        macro_rules! store_waker {
            ($should_flush:expr) => {
                // Store the waker, in order to be able to wakeup the caller
                // when the sender state changes
                if let Some(waker) = context.as_ref().map(|context| context.waker().clone()) {
                    let should_flush = $should_flush || {
                        // persist existing flush requests
                        self.write_waiter
                            .take()
                            .map_or(false, |(_, should_flush)| should_flush)
                    };
                    self.write_waiter = Some((waker, should_flush));
                    response.will_wake = true;
                }
            };
        }

        if let Some(error_code) = request.reset {
            // reset is a best effort operation so ignore the result
            let _ = self.init_reset(
                ResetSource::LocalApplication,
                StreamError::stream_reset(error_code),
            );

            // mark the stream as resetting
            response.status = ops::Status::Resetting;

            if request.flush && !matches!(self.state, SendStreamState::ResetAcknowledged(_)) {
                // the request wanted to wait until the reset was ACKed to unblock
                store_waker!(true);
            } else {
                // clear any previously registered waiters since the stream is now closed
                self.write_waiter = None;
            }

            // Mark the stream as completely reset once it's been acknowledged
            if matches!(self.state, SendStreamState::ResetAcknowledged(_)) {
                response.status = ops::Status::Reset(StreamError::stream_reset(error_code));
            }

            return Ok(response);
        }

        if request.reset_on_loss {
            self.reset_on_loss = true;
        }

        // Do some state checks here. Only write data when the client is still
        // allowed to write (not reset).
        match self.state {
            SendStreamState::ResetSent(error) | SendStreamState::ResetAcknowledged(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                self.write_waiter = None;
                return Err(error);
            }
            SendStreamState::Sending => {
                // continue
            }
        }

        if let Some(chunks) = request.chunks.as_mut().filter(|chunks| !chunks.is_empty()) {
            for chunk in chunks.iter_mut() {
                // empty chunks are automatically consumed
                if chunk.is_empty() {
                    response.chunks.consumed += 1;
                    continue;
                }

                self.validate_push(chunk.len())?;

                if !self.can_push() {
                    store_waker!(false);

                    // no more progress can be made on the operation
                    return Ok(response);
                }

                response.bytes.consumed += chunk.len();
                response.chunks.consumed += 1;

                self.data_sender
                    .push(core::mem::replace(chunk, Bytes::new()));
            }
        } else if !request.finish && !request.flush && context.is_some() {
            // if `chunks` are `None` or `Some(&[])` and we're not ending or flushing the stream,
            // the caller is only interested in notifications of state changes.

            // test a potential push of 1 byte
            self.validate_push(1)?;

            // store the waker if we currently can't push
            if !self.can_push() {
                store_waker!(false);

                return Ok(response);
            }
        }

        if request.finish {
            match self.data_sender.state() {
                data_sender::State::Finished => {
                    // All packets incl. the one with the FIN flag had been acknowledged.
                    // => We are done with this stream!
                    // We just record here that the user actually has retrieved the
                    // information.
                    self.final_state_observed = true;
                    // clear any previously register waiters
                    self.write_waiter = None;
                    response.status = ops::Status::Finished;
                    return Ok(response);
                }
                _ => {
                    self.data_sender.finish();
                    response.status = ops::Status::Finishing;

                    if request.flush {
                        // block the request until the peer has ACKed the last frame
                        store_waker!(true);
                    } else {
                        // clear any previously registered waiters
                        self.write_waiter = None;
                    }
                }
            }
        } else if request.flush && !self.data_sender.is_empty() {
            // notify callers once the buffer has been flushed
            store_waker!(true);
        }

        match self.data_sender.state() {
            data_sender::State::Sending => {
                // inform the caller of the available space to send
                response.bytes.available = self.data_sender.available_buffer_space();
                // assume chunks are 1 bytes
                response.chunks.available = response.bytes.available;
            }
            data_sender::State::Finishing(_) => {
                response.status = ops::Status::Finishing;
            }
            data_sender::State::Finished => {
                response.status = ops::Status::Finished;
            }
            data_sender::State::Cancelled(error) => {
                // TODO determine if the peer has acknowledged the reset
                response.status = ops::Status::Reset(error);
            }
        }

        Ok(response)
    }

    fn detach(&mut self) {
        self.detached = true;
        self.write_waiter = None;

        match &self.state {
            // if the RESET_STREAM frame has been ACKed then we can finalize the stream
            SendStreamState::ResetAcknowledged(_) => {
                self.final_state_observed = true;
            }
            // If we are finished sending and the application isn't subscribed to updates
            SendStreamState::Sending => {
                self.final_state_observed |=
                    matches!(self.data_sender.state(), data_sender::State::Finished);
            }
            _ => {}
        }
    }

    /// Returns true if the caller can push additional data
    fn can_push(&self) -> bool {
        // We accept the data if there is at least 1 byte of space
        // available in the flow control window.
        self.data_sender.available_buffer_space() > 0
    }

    /// Ensures a potential push operation would be valid
    fn validate_push(&self, len: usize) -> Result<(), StreamError> {
        // The user tries to write, even though they previously closed the stream.
        if self.data_sender.state() != data_sender::State::Sending {
            return Err(StreamError::send_after_finish());
        }

        let is_valid = VarInt::try_from(len)
            .ok()
            .and_then(|chunk_len| self.data_sender.total_enqueued_len().checked_add(chunk_len))
            .is_some();

        if is_valid {
            Ok(())
        } else {
            Err(StreamError::max_stream_data_size_exceeded())
        }
    }

    /// Starts the reset procedure if the Stream has not been in a RESET state
    /// before. The method will return whether calling this method caused the
    /// `Stream` to enter a RESET state.
    fn init_reset(&mut self, reason: ResetSource, error: StreamError) -> InitResetResult {
        match self.state {
            SendStreamState::ResetSent(_) | SendStreamState::ResetAcknowledged(_) => {
                return InitResetResult::ResetNotNecessary
            }
            SendStreamState::Sending
                if self.data_sender.state() == data_sender::State::Finished =>
            {
                return InitResetResult::ResetNotNecessary
            }
            SendStreamState::Sending => {}
        }

        self.state = if reason.is_internal() {
            // Internal Resets do not require an ACK
            SendStreamState::ResetAcknowledged(error)
        } else {
            SendStreamState::ResetSent(error)
        };

        // If the application initiated the reset it is aware about the
        // final state
        if reason.is_local_application() {
            self.final_state_observed = true;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.1
        //# An endpoint MAY send a RESET_STREAM as the first frame that mentions
        //# a stream; this causes the sending part of that stream to open and
        //# then immediately transition to the "Reset Sent" state.

        // Clear the send buffer. Since we initiated a RESET, there is no need
        // to send or resend the remaining data.
        self.data_sender.stop_sending(error);

        // For an internal reset (which provides no error_code) we do not need
        // to transmit the reset frame
        match (reason.is_internal(), error) {
            (false, StreamError::StreamReset { error, .. }) => {
                // When we deliver a RESET frame, we have to transmit the final
                // size of the stream. This is required to keep the connection
                // window on both sides in sync.
                // The `acquired_connection_flow_controller_window()` method
                // returns how much of the window we have reserved for this
                // Stream and can not use for other Streams. Therefore we
                // deliver this value to the peer - even if we have actually
                // transmitted less data actually.
                self.reset_sync.request_delivery(OutgoingResetData {
                    application_error_code: error,
                    final_size: self
                        .data_sender
                        .flow_controller()
                        .acquired_connection_flow_controller_window(),
                });
            }
            (false, _) => {
                unreachable!("Non internal reasons must be accommodated by an error code")
            }
            (true, _) => {
                self.reset_sync.stop_sync();
            }
        };

        InitResetResult::ResetInitiated
    }
}

impl timer::Provider for SendStream {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.data_sender.flow_controller().timers(query)?;
        Ok(())
    }
}

impl StreamInterestProvider for SendStream {
    #[inline]
    fn stream_interests(&self, interests: &mut StreamInterests) {
        match self.state {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-3.3
            //# A sender MUST NOT send any of these frames from a terminal state
            //# ("Data Recvd" or "Reset Recvd").
            SendStreamState::Sending if self.final_state_observed => {
                return;
            }
            SendStreamState::ResetAcknowledged(_) => {
                if self.final_state_observed {
                    return;
                }
            }
            //= https://www.rfc-editor.org/rfc/rfc9000#section-3.3
            //# A sender MUST NOT send a STREAM or
            //# STREAM_DATA_BLOCKED frame for a stream in the "Reset Sent" state or
            //# any terminal state -- that is, after sending a RESET_STREAM frame.
            SendStreamState::ResetSent(_) => {
                interests.with_transmission(|query| self.reset_sync.transmission_interest(query))
            }
            _ => interests.with_transmission(|query| {
                self.data_sender.transmission_interest(query)?;
                self.data_sender
                    .flow_controller()
                    .transmission_interest(query)?;
                self.reset_sync.transmission_interest(query)?;
                Ok(())
            }),
        }

        // let the stream container know we still have work to do
        interests.retained = true;

        // Check whether the flow controller reports being blocked on the
        // connection flow control window or the stream flow control window
        match self.data_sender.flow_controller().state() {
            StreamFlowControllerState::BlockedOnStreamWindow => {
                interests.stream_flow_control_credits = true
            }
            StreamFlowControllerState::BlockedOnConnectionWindow => {
                interests.connection_flow_control_credits = true
            }
            _ => {}
        }

        interests.delivery_notifications |=
            self.data_sender.is_inflight() || self.reset_sync.is_inflight();
    }
}

#[cfg(test)]
mod tests;
