use crate::{
    contexts::{ConnectionContext, OnTransmitError, WriteContext},
    frame_exchange_interests::FrameExchangeInterestProvider,
    stream::{
        outgoing_connection_flow_controller::OutgoingConnectionFlowController,
        stream_events::StreamEvents,
        stream_interests::{StreamInterestProvider, StreamInterests},
        StreamError,
    },
    sync::{
        ChunkToFrameWriter, DataSender, DataSenderState, OnceSync, OutgoingDataFlowController,
        ValueToFrameWriter,
    },
};
use bytes::Bytes;
use core::task::{Context, Poll, Waker};
use s2n_quic_core::{
    ack_set::AckSet,
    application::ApplicationErrorCode,
    frame::{stream::StreamRef, MaxPayloadSizeForFrame, MaxStreamData, ResetStream, StopSending},
    packet::number::PacketNumber,
    stream::StreamId,
    transport::error::TransportError,
    varint::VarInt,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#3.1
//# Figure 1 shows the states for the part of a stream that sends data to
//# a peer.
//#
//#        o
//#        | Create Stream (Sending)
//#        | Peer Creates Bidirectional Stream
//#        v
//#    +-------+
//#    | Ready | Send RESET_STREAM
//#    |       |-----------------------.
//#    +-------+                       |
//#        |                           |
//#        | Send STREAM /             |
//#        |      STREAM_DATA_BLOCKED  |
//#        |                           |
//#        | Peer Creates              |
//#        |      Bidirectional Stream |
//#        v                           |
//#    +-------+                       |
//#    | Send  | Send RESET_STREAM     |
//#    |       |---------------------->|
//#    +-------+                       |
//#        |                           |
//#        | Send STREAM + FIN         |
//#        v                           v
//#    +-------+                   +-------+
//#    | Data  | Send RESET_STREAM | Reset |
//#    | Sent  |------------------>| Sent  |
//#    +-------+                   +-------+
//#        |                           |
//#        | Recv All ACKs             | Recv ACK
//#        v                           v
//#    +-------+                   +-------+
//#    | Data  |                   | Reset |
//#    | Recvd |                   | Recvd |
//#    +-------+                   +-------+
//#
//#             Figure 1: States for Sending Parts of Streams
//#
//# The sending part of stream that the endpoint initiates (types 0 and 2
//# for clients, 1 and 3 for servers) is opened by the application.  The
//# "Ready" state represents a newly created stream that is able to
//# accept data from the application.  Stream data might be buffered in
//# this state in preparation for sending.
//#
//# Sending the first STREAM or STREAM_DATA_BLOCKED frame causes a
//# sending part of a stream to enter the "Send" state.  An
//# implementation might choose to defer allocating a stream ID to a
//# stream until it sends the first STREAM frame and enters this state,
//# which can allow for better stream prioritization.
//#
//# The sending part of a bidirectional stream initiated by a peer (type
//# 0 for a server, type 1 for a client) enters the "Ready" state then
//# immediately transitions to the "Send" state if the receiving part
//# enters the "Recv" state (Section 3.2).
//#
//# In the "Send" state, an endpoint transmits - and retransmits as
//# necessary - stream data in STREAM frames.  The endpoint respects the
//# flow control limits set by its peer, and continues to accept and
//# process MAX_STREAM_DATA frames.  An endpoint in the "Send" state
//# generates STREAM_DATA_BLOCKED frames if it is blocked from sending by
//# stream or connection flow control limits Section 4.1.
//#
//# After the application indicates that all stream data has been sent
//# and a STREAM frame containing the FIN bit is sent, the sending part
//# of the stream enters the "Data Sent" state.  From this state, the
//# endpoint only retransmits stream data as necessary.  The endpoint
//# does not need to check flow control limits or send
//# STREAM_DATA_BLOCKED frames for a stream in this state.
//# MAX_STREAM_DATA frames might be received until the peer receives the
//# final stream offset.  The endpoint can safely ignore any
//# MAX_STREAM_DATA frames it receives from its peer for a stream in this
//# state.
//#
//# Once all stream data has been successfully acknowledged, the sending
//# part of the stream enters the "Data Recvd" state, which is a terminal
//# state.
//#
//# From any of the "Ready", "Send", or "Data Sent" states, an
//# application can signal that it wishes to abandon transmission of
//# stream data.  Alternatively, an endpoint might receive a STOP_SENDING
//# frame from its peer.  In either case, the endpoint sends a
//# RESET_STREAM frame, which causes the stream to enter the "Reset Sent"
//# state.
//#
//# An endpoint MAY send a RESET_STREAM as the first frame that mentions
//# a stream; this causes the sending part of that stream to open and
//# then immediately transition to the "Reset Sent" state.
//#
//# Once a packet containing a RESET_STREAM has been acknowledged, the
//# sending part of the stream enters the "Reset Recvd" state, which is a
//# terminal state.

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
    application_error_code: ApplicationErrorCode,
}

/// Writes the `RESET` frames based on the streams flow control window.
#[derive(Default)]
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

/// Serializes and writes `Stream` frames
#[derive(Default)]
pub struct StreamChunkToFrameWriter {}

impl ChunkToFrameWriter for StreamChunkToFrameWriter {
    type StreamId = StreamId;

    fn get_max_frame_size(&self, stream_id: Self::StreamId, data_size: usize) -> usize {
        StreamRef::get_max_frame_size(stream_id.into(), data_size)
    }

    fn max_payload_size(
        &self,
        stream_id: Self::StreamId,
        max_frame_size: usize,
        offset: VarInt,
    ) -> MaxPayloadSizeForFrame {
        StreamRef::max_payload_size(max_frame_size, stream_id.into(), offset)
    }

    fn write_value_as_frame<W: WriteContext>(
        &self,
        stream_id: Self::StreamId,
        offset: VarInt,
        data: &[u8],
        is_last_frame: bool,
        is_fin: bool,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&StreamRef {
            stream_id: stream_id.into(),
            offset,
            is_last_frame,
            data,
            is_fin,
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
    // /// Whether the sender is blocked and can not transmit data,
    // /// even though data is available.
    // blocked: Option<SenderBlockedReason>,
    state: StreamFlowControllerState,
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
        }
    }

    /// Updates the `MAXIMUM_STREAM_DATA` value which was communicated by a peer
    pub fn set_max_stream_data(&mut self, max_stream_data: VarInt) {
        if max_stream_data <= self.max_stream_data {
            // We are only interested in window increments
            return;
        }

        self.max_stream_data = max_stream_data;
        if self.state == StreamFlowControllerState::BlockedOnStreamWindow {
            self.state = StreamFlowControllerState::Ready;
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
}

impl OutgoingDataFlowController for StreamFlowController {
    fn acquire_flow_control_window(&mut self, min_offset: VarInt, size: usize) -> VarInt {
        debug_assert_ne!(
            StreamFlowControllerState::Finished,
            self.state,
            "Trying to acquire window from finished stream"
        );
        if self.state == StreamFlowControllerState::Finished {
            return self.available_window();
        }
        self.state = StreamFlowControllerState::Ready;

        if min_offset > self.max_stream_data {
            // Can't send any data due to being blocked on the Stream window
            // self.state = Some(SenderBlockedReason::StreamFlowControl);
            self.state = StreamFlowControllerState::BlockedOnStreamWindow;
            return self.available_window();
        }

        if min_offset == self.max_stream_data && size > 0 {
            // Also can't send data due to being blocked on the Stream window
            // self.blocked = Some(SenderBlockedReason::StreamFlowControl);
            self.state = StreamFlowControllerState::BlockedOnStreamWindow;
            return self.available_window();
        }

        let end_offset = min_offset + size;
        self.highest_requested_connection_flow_control_window = core::cmp::max(
            end_offset,
            self.highest_requested_connection_flow_control_window,
        );
        self.try_acquire_connection_window();

        if min_offset > self.acquired_connection_flow_controller_window {
            // Can't send due to being blocked on the connection flow control window
            // self.blocked = Some(SenderBlockedReason::ConnectionFlowControl);
            self.state = StreamFlowControllerState::BlockedOnConnectionWindow;
            return self.available_window();
        }

        if min_offset == self.acquired_connection_flow_controller_window && size > 0 {
            // Can't send due to being blocked on the connection flow control window
            // self.blocked = Some(SenderBlockedReason::ConnectionFlowControl);
            self.state = StreamFlowControllerState::BlockedOnConnectionWindow;
            return self.available_window();
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
    }

    fn finish(&mut self) {
        self.state = StreamFlowControllerState::Finished;
    }
}

/// The sending half of a stream
pub struct SendStream {
    /// The current state of the stream
    pub(super) state: SendStreamState,
    /// Transmitter for outgoing data
    pub(super) data_sender: DataSender<StreamFlowController, StreamChunkToFrameWriter>,
    /// Synchronizes sending a `RESET` to the receiver
    pub(super) reset_sync: OnceSync<OutgoingResetData, ResetStreamToFrameWriter>,
    /// The handle of a task that is currently waiting on new incoming data or
    /// on waiting for the finalization process to complete
    pub(super) write_waiter: Option<Waker>,
    /// Whether the final state had already been observed by the application
    final_state_observed: bool,
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
    ) -> Result<(), TransportError> {
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
                && self.data_sender.state() == DataSenderState::Sending
            {
                if let Some(waker) = self.write_waiter.take() {
                    events.store_write_waker(waker);
                }
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
    ) -> Result<(), TransportError> {
        //=https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#3.5
        //# A STOP_SENDING frame requests that the receiving endpoint send a
        //# RESET_STREAM frame.  An endpoint that receives a STOP_SENDING frame
        //# MUST send a RESET_STREAM frame if the stream is in the Ready or Send
        //# state.  If the stream is in the Data Sent state and any outstanding
        //# data is declared lost, an endpoint SHOULD send a RESET_STREAM frame
        //# in lieu of a retransmission.

        // We do not track whether we transmit a range for the first time or
        // whether it is a retransmit. Therefore we can not decide whether the
        // oustanding data is lost, or has never been transmitted.
        // Assuming the client really does not want any data anymore, we will
        // still simply emit a RESET in all cases.

        // Under these assumptions the reset will happen in exactly the same
        // cases as if the user of the `Stream` would have triggered a reset,
        // and we can delegate to the same method.

        if self.init_reset(
            ResetSource::StopSendingFrame,
            StreamError::StreamReset(frame.application_error_code.into()),
        ) == InitResetResult::ResetInitiated
        {
            // Return the waker to wake up potential users of the stream.
            // If the Stream got reset, then blocked writers need to get woken up.
            if let Some(waker) = self.write_waiter.take() {
                events.store_write_waker(waker);
            }
        }

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A, events: &mut StreamEvents) {
        self.data_sender.on_packet_ack(ack_set);

        if let SendStreamState::Sending = self.state {
            if self.data_sender.state() == DataSenderState::Sending {
                // In this state we have to wake up the user if the they can
                // queue more data for transmission. This is possible if
                // acknowledging packets removed them from the send queue and
                // we got additional space in the send queue.
                if self.data_sender.available_buffer_space() > 0 {
                    if let Some(waker) = self.write_waiter.take() {
                        events.store_write_waker(waker);
                    }
                }
            } else if self.data_sender.state() == DataSenderState::FinishAcknowledged {
                // If we have already sent a fin and just waiting for it to be
                // acknowledged, `on_packet_ack` might have moved us into the
                // final state due.
                //
                // In this state we also wake up the application,
                // since the finish operation has been confirmed.
                if let Some(waker) = self.write_waiter.take() {
                    events.store_write_waker(waker);
                }
            }
        }

        self.reset_sync.on_packet_ack(ack_set);

        if let SendStreamState::ResetSent(error_code) = self.state {
            if self.reset_sync.is_delivered() {
                // A reset had been acknowledged. Enter the terminal state.
                self.state = SendStreamState::ResetAcknowledged(error_code);
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        self.data_sender.on_packet_loss(ack_set);
        self.reset_sync.on_packet_loss(ack_set);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.reset_sync.on_transmit(stream_id, context)?;
        self.data_sender.on_transmit(stream_id, context)
    }

    /// A reset that is triggered without having received a `RESET` frame.
    pub fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents) {
        if self.init_reset(
            // This is remote in a sense we do not have to emit a message
            ResetSource::InternalReset,
            error,
        ) == InitResetResult::ResetInitiated
        {
            // Return the waker to wake up potential users of the stream.
            // If the Stream got reset, then blocked writers need to get woken up.
            if let Some(waker) = self.write_waiter.take() {
                events.store_write_waker(waker);
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

    // These functions are called from the client API

    /// Tries to enqueue data for transmission on the `Stream`.
    ///
    /// The method will succeed as long as buffering space is available for the data,
    /// and as long as the `Stream` is still in the `Sending` state. If `finish()`
    /// had been called before, or if the `Stream` was reset in between, the
    /// method will return a [`StreamError`].
    ///
    /// TODO: `poll_push()` does not return the data on sending errors. Should it?
    /// YES, it must. Otherwise the client can not really retry.
    pub fn poll_push<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        data: Bytes,
        context: &Context,
    ) -> Poll<Result<(), StreamError>> {
        // Do some state checks here. Only read data when the client is still
        // allowed to read (not reset).

        match self.state {
            SendStreamState::ResetSent(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                Poll::Ready(Err(error))
            }
            SendStreamState::ResetAcknowledged(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                Poll::Ready(Err(error))
            }
            SendStreamState::Sending => {
                if self.data_sender.state() != DataSenderState::Sending {
                    // The user tries to write, even though they previously closed
                    // the stream.
                    return Poll::Ready(Err(StreamError::SendAfterFinish));
                }

                // 0byte writes are always possible, because we don't have to
                // do anything.
                if data.is_empty() {
                    return Poll::Ready(Ok(()));
                }

                // We can never write more than the maximum possible send window
                let data_size_varint = if let Ok(varint) = VarInt::new(data.len() as u64) {
                    varint
                } else {
                    return Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded));
                };
                if self
                    .data_sender
                    .total_enqueued_len()
                    .checked_add(data_size_varint)
                    .is_none()
                {
                    return Poll::Ready(Err(StreamError::MaxStreamDataSizeExceeded));
                }

                // We accept the data if there is at least 1 byte of space
                // available in the flow control window.
                let available_space = self.data_sender.available_buffer_space();
                if available_space < 1 {
                    // Store the waker, in order to be able to wakeup the client
                    // when new buffering capacity is available.
                    self.write_waiter = Some(context.waker().clone());
                    return Poll::Pending;
                }

                self.data_sender.push(data);

                Poll::Ready(Ok(()))
            }
        }
    }

    /// Starts the finalization process of a `Stream` and queries for the progress
    /// of it.
    ///
    /// The first call to [`finish()`] will start the finalization process. In this
    /// case the `FIN` flag for the `Stream` will be transmitted to the peer as
    /// soon as possible. The method will return `Poll::Pending` as long as not
    /// all outgoing data has been acknowledged by the peer. As soon as all data
    /// had been acknowledged the method will return `Poll::Ready(())`.
    ///
    /// If the `Stream` gets reset while waiting for acknowledgement of all
    /// outstanding data the method will return a [`StreamError`].
    pub fn poll_finish<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        context: &Context,
    ) -> Poll<Result<(), StreamError>> {
        match self.state {
            SendStreamState::ResetSent(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                Poll::Ready(Err(error))
            }
            SendStreamState::ResetAcknowledged(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                Poll::Ready(Err(error))
            }
            SendStreamState::Sending => {
                if self.data_sender.state() == DataSenderState::FinishAcknowledged {
                    // All packets incl. the one with the FIN flag had been acknowledged.
                    // => We are done with this stream!
                    // We just record here that the user actually has retrieved the
                    // information.
                    self.final_state_observed = true;
                    return Poll::Ready(Ok(()));
                }

                // Store the waker, in order to be able to wakeup the client
                // when the finish gets acknowledged.
                self.write_waiter = Some(context.waker().clone());

                self.data_sender.finish();

                Poll::Pending
            }
        }
    }

    /// Initiates a reset of the Stream from the application side
    pub fn reset<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        error_code: ApplicationErrorCode,
    ) -> Result<(), StreamError> {
        // The result is not important in this case. Since the reset had been
        // initiated by a user API call, we do not have to wake up another user
        // API call (like `push` or `finish`).
        let _init_reset_result = self.init_reset(
            ResetSource::LocalApplication,
            StreamError::StreamReset(error_code),
        );

        Ok(())
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
                if self.data_sender.state() == DataSenderState::FinishAcknowledged =>
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
        };

        // Clear the send buffer. Since we initiated a RESET, there is no need
        // to send or resend the remaining data.
        self.data_sender.stop_sending();

        // For an internal reset (which provides no error_code) we do not need
        // to transmit the reset frame
        match (reason.is_internal(), error) {
            (false, StreamError::StreamReset(error_code)) => {
                // When we deliver a RESET frame, we have to transmit the final
                // size of the stream. This is required to keep the connection
                // window on both sides in sync.
                // The `acquired_connection_flow_controller_window()` method
                // returns how much of the window we have reserved for this
                // Stream and can not use for other Streams. Therefore we
                // deliver this value to the peer - even if we have actually
                // transmitted less data actually.
                self.reset_sync.request_delivery(OutgoingResetData {
                    application_error_code: error_code,
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

impl StreamInterestProvider for SendStream {
    fn interests(&self) -> StreamInterests {
        let finalization = self.final_state_observed
            && match self.state {
                SendStreamState::Sending => {
                    // In this state `final_state_observed` will only be set
                    // when all data has actually been transmitted. Therefore we
                    // don't need an extra check based on the writers state.
                    true
                }
                SendStreamState::ResetAcknowledged(_) => true,
                _ => false,
            };

        // Check whether the flow controller reports being blocked on the
        // connection flow control window
        let connection_flow_control_credits = self.data_sender.flow_controller().state()
            == StreamFlowControllerState::BlockedOnConnectionWindow;

        StreamInterests {
            finalization,
            connection_flow_control_credits,
            frame_exchange: self.data_sender.frame_exchange_interests()
                + self.reset_sync.frame_exchange_interests(),
        }
    }
}
