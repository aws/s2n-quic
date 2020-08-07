use crate::{
    buffer::{StreamReceiveBuffer, StreamReceiveBufferError},
    contexts::{ConnectionContext, OnTransmitError, WriteContext},
    frame_exchange_interests::FrameExchangeInterestProvider,
    stream::{
        incoming_connection_flow_controller::IncomingConnectionFlowController,
        stream_events::StreamEvents,
        stream_interests::{StreamInterestProvider, StreamInterests},
        StreamError,
    },
    sync::{IncrementalValueSync, OnceSync, ValueToFrameWriter},
};
use bytes::Bytes;
use core::task::{Context, Poll, Waker};
use s2n_quic_core::{
    ack_set::AckSet,
    application::ApplicationErrorCode,
    frame::{stream::StreamRef, MaxStreamData, ResetStream, StopSending, StreamDataBlocked},
    packet::number::PacketNumber,
    stream::StreamId,
    transport::error::TransportError,
    varint::VarInt,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#3.2
//# Figure 2 shows the states for the part of a stream that receives data
//# from a peer.  The states for a receiving part of a stream mirror only
//# some of the states of the sending part of the stream at the peer.
//# The receiving part of a stream does not track states on the sending
//# part that cannot be observed, such as the "Ready" state.  Instead,
//# the receiving part of a stream tracks the delivery of data to the
//# application, some of which cannot be observed by the sender.
//#
//#        o
//#        | Recv STREAM / STREAM_DATA_BLOCKED / RESET_STREAM
//#        | Create Bidirectional Stream (Sending)
//#        | Recv MAX_STREAM_DATA / STOP_SENDING (Bidirectional)
//#        | Create Higher-Numbered Stream
//#        v
//#    +-------+
//#    | Recv  | Recv RESET_STREAM
//#    |       |-----------------------.
//#    +-------+                       |
//#        |                           |
//#        | Recv STREAM + FIN         |
//#        v                           |
//#    +-------+                       |
//#    | Size  | Recv RESET_STREAM     |
//#    | Known |---------------------->|
//#    +-------+                       |
//#        |                           |
//#        | Recv All Data             |
//#        v                           v
//#    +-------+ Recv RESET_STREAM +-------+
//#    | Data  |--- (optional) --->| Reset |
//#    | Recvd |  Recv All Data    | Recvd |
//#    +-------+<-- (optional) ----+-------+
//#        |                           |
//#        | App Read All Data         | App Read RST
//#        v                           v
//#    +-------+                   +-------+
//#    | Data  |                   | Reset |
//#    | Read  |                   | Read  |
//#    +-------+                   +-------+
//#
//#            Figure 2: States for Receiving Parts of Streams
//#
//# The receiving part of a stream initiated by a peer (types 1 and 3 for
//# a client, or 0 and 2 for a server) is created when the first STREAM,
//# STREAM_DATA_BLOCKED, or RESET_STREAM is received for that stream.
//# For bidirectional streams initiated by a peer, receipt of a
//# MAX_STREAM_DATA or STOP_SENDING frame for the sending part of the
//# stream also creates the receiving part.  The initial state for the
//# receiving part of stream is "Recv".
//#
//# The receiving part of a stream enters the "Recv" state when the
//# sending part of a bidirectional stream initiated by the endpoint
//# (type 0 for a client, type 1 for a server) enters the "Ready" state.
//#
//# An endpoint opens a bidirectional stream when a MAX_STREAM_DATA or
//# STOP_SENDING frame is received from the peer for that stream.
//# Receiving a MAX_STREAM_DATA frame for an unopened stream indicates
//# that the remote peer has opened the stream and is providing flow
//# control credit.  Receiving a STOP_SENDING frame for an unopened
//# stream indicates that the remote peer no longer wishes to receive
//# data on this stream.  Either frame might arrive before a STREAM or
//# STREAM_DATA_BLOCKED frame if packets are lost or reordered.
//#
//# Before a stream is created, all streams of the same type with lower-
//# numbered stream IDs MUST be created.  This ensures that the creation
//# order for streams is consistent on both endpoints.
//#
//# In the "Recv" state, the endpoint receives STREAM and
//# STREAM_DATA_BLOCKED frames.  Incoming data is buffered and can be
//# reassembled into the correct order for delivery to the application.
//# As data is consumed by the application and buffer space becomes
//# available, the endpoint sends MAX_STREAM_DATA frames to allow the
//# peer to send more data.
//#
//# When a STREAM frame with a FIN bit is received, the final size of the
//# stream is known (see Section 4.4).  The receiving part of the stream
//# then enters the "Size Known" state.  In this state, the endpoint no
//# longer needs to send MAX_STREAM_DATA frames, it only receives any
//# retransmissions of stream data.
//#
//# Once all data for the stream has been received, the receiving part
//# enters the "Data Recvd" state.  This might happen as a result of
//# receiving the same STREAM frame that causes the transition to "Size
//# Known".  After all data has been received, any STREAM or
//# STREAM_DATA_BLOCKED frames for the stream can be discarded.
//#
//# The "Data Recvd" state persists until stream data has been delivered
//# to the application.  Once stream data has been delivered, the stream
//# enters the "Data Read" state, which is a terminal state.
//#
//# Receiving a RESET_STREAM frame in the "Recv" or "Size Known" states
//# causes the stream to enter the "Reset Recvd" state.  This might cause
//# the delivery of stream data to the application to be interrupted.
//#
//# It is possible that all stream data is received when a RESET_STREAM
//# is received (that is, from the "Data Recvd" state).  Similarly, it is
//# possible for remaining stream data to arrive after receiving a
//# RESET_STREAM frame (the "Reset Recvd" state).  An implementation is
//# free to manage this situation as it chooses.
//#
//# Sending RESET_STREAM means that an endpoint cannot guarantee delivery
//# of stream data; however there is no requirement that stream data not
//# be delivered if a RESET_STREAM is received.  An implementation MAY
//# interrupt delivery of stream data, discard any data that was not
//# consumed, and signal the receipt of the RESET_STREAM.  A RESET_STREAM
//# signal might be suppressed or withheld if stream data is completely
//# received and is buffered to be read by the application.  If the
//# RESET_STREAM is suppressed, the receiving part of the stream remains
//# in "Data Recvd".
//#
//# Once the application receives the signal indicating that the stream
//# was reset, the receiving part of the stream transitions to the "Reset
//# Read" state, which is a terminal state.

/// Enumerates the possible states of the receiving side of a stream.
/// These states are equivalent to the ones in the QUIC transport specification.
#[derive(PartialEq, Debug, Copy, Clone)]
pub(super) enum ReceiveStreamState {
    /// The stream is still receiving data from the remote peer. This state
    /// coverst the `Recv`, `Size Known` and `Data Recvd` state from the QUIC
    /// specification. These are modelled as a single state because the handling
    /// for the states is mostly identical.
    /// The parameter indicates the total size of the stream if it had already
    /// been signalled by the peer.
    Receiving(Option<u64>),
    /// All data had been received from the peer and consumed by the user.
    /// This is the terminal state.
    DataRead,
    /// The connection was reset. The flag indicates whether the reset status
    /// had already been observed by the user.
    Reset(StreamError),
}

/// Writes the `MAX_STREAM_DATA` frames based on the streams flow control window.
#[derive(Default)]
pub(super) struct MaxStreamDataToFrameWriter {}

impl ValueToFrameWriter<VarInt> for MaxStreamDataToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&MaxStreamData {
            stream_id: stream_id.into(),
            maximum_stream_data: value,
        })
    }
}

/// Writes `STOP_SENDING` frames basd on `ApplicationErrorCode`s
#[derive(Default)]
pub(super) struct StopSendingToFrameWriter {}

impl ValueToFrameWriter<ApplicationErrorCode> for StopSendingToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: ApplicationErrorCode,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&StopSending {
            stream_id: stream_id.into(),
            application_error_code: value.into(),
        })
    }
}

/// A composite flow controller for receiving data.
/// The flow controller manages the Streams individual window as well as the
/// connection flow control window.
pub(super) struct ReceiveStreamFlowController {
    /// The connection flow controller
    pub(super) connection_flow_controller: IncomingConnectionFlowController,
    /// Synchronizes the read window to the remote peer
    pub(super) read_window_sync: IncrementalValueSync<VarInt, MaxStreamDataToFrameWriter>,
    /// The relative flow control window we want to maintain
    pub(super) desired_flow_control_window: u32,
    /// The amount of credits which had been acquired from the connection and
    /// stream window in total
    pub(super) acquired_connection_window: VarInt,
    /// The amount of credits which had been released in total
    pub(super) released_connection_window: VarInt,
}

impl ReceiveStreamFlowController {
    fn new(
        connection_flow_controller: IncomingConnectionFlowController,
        initial_window: VarInt,
        desired_flow_control_window: u32,
    ) -> Self {
        Self {
            connection_flow_controller,
            read_window_sync: IncrementalValueSync::new(
                VarInt::from_u32(desired_flow_control_window),
                initial_window,
                VarInt::from_u32(desired_flow_control_window / 10),
            ),
            acquired_connection_window: VarInt::from_u32(0),
            released_connection_window: VarInt::from_u32(0),
            desired_flow_control_window,
        }
    }

    /// Asserts that the flow control window up to the given offset is available
    ///
    /// This checks the Streams individual flow control limit as well as the
    /// connections flow control limit.
    /// For the connections limit the method will acquire the necessary remaining
    /// limit from the connetions flow controller.
    fn acquire_window_up_to(
        &mut self,
        offset: VarInt,
        source_frame_type: Option<u8>,
    ) -> Result<(), TransportError> {
        // Step 1: Check the stream limit

        //=https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#19.10
        //# The data sent on a stream MUST NOT exceed the largest maximum stream
        //# data value advertised by the receiver.  An endpoint MUST terminate a
        //# connection with a FLOW_CONTROL_ERROR error if it receives more data
        //# than the largest maximum stream data that it has sent for the
        //# affected stream, unless this is a result of a change in the initial
        //# limits (see Section 7.3.1).
        if offset > self.read_window_sync.latest_value() {
            return Err(TransportError::FLOW_CONTROL_ERROR
                .with_reason("Stream flow control window exceeded")
                .with_optional_frame_type(source_frame_type.map(|ty| ty.into())));
        }
        // Remark: Actually this read window might not have yet been
        // transmitted to the peer. In that case it might have now
        // successfully sent us data even though we didn't request
        // yet. However even if we knew we sent the MAX_STREAM_DATA frame
        // we wouldn't knew whether the peer actually received it and
        // send their data because of that. Therefore there exists
        // always some uncertainity around the window. The most
        // important part however is that the client can never send
        // us any data outside of a given window - which is still
        // enforced here.

        // Step 2: Check the connection limit
        // Take into account that we might already have acquired a higher
        // connection window than what is necessary for the given offset
        let additional_connection_window = offset.saturating_sub(self.acquired_connection_window);
        if additional_connection_window > VarInt::from_u32(0) {
            self.connection_flow_controller
                .acquire_window(additional_connection_window)
                .map_err(|_| {
                    TransportError::FLOW_CONTROL_ERROR
                        .with_reason("Connection flow control window exceeded")
                        .with_optional_frame_type(source_frame_type.map(|ty| ty.into()))
                })?;
            // The connection window was acquired successfully
            self.acquired_connection_window += additional_connection_window;
        }

        Ok(())
    }

    /// Notifies the flow controller that the relative amount of data had been
    /// consumed from the Stream.
    ///
    /// The flow controller will use the information to enqueue window updates
    /// if necessary
    fn release_window(&mut self, amount: VarInt) {
        self.released_connection_window += amount;

        // Enqueue Stream window updates by increasing the latest value on
        // the read window synchronisation component
        self.read_window_sync.update_latest_value(
            self.released_connection_window
                .saturating_add(VarInt::from_u32(self.desired_flow_control_window)),
        );

        // Notify the connection flow controller about the consumed data
        self.connection_flow_controller.release_window(amount);
    }

    /// Releases all flow credits which had been acquired but not yet released
    /// through previous [`release_window`] calls.
    fn release_outstanding_window(&mut self) {
        let unreleased = self.acquired_connection_window - self.released_connection_window;
        self.release_window(unreleased);
    }

    /// Stop to synchronize the Streams flow control window to the peer
    fn stop_sync(&mut self) {
        self.read_window_sync.stop_sync();
    }

    /// Returns the MAX_STREAM_DATA window that is currently synchronized
    /// towards the peer.
    #[cfg(test)]
    pub(super) fn current_stream_receive_window(&self) -> VarInt {
        self.read_window_sync.latest_value()
    }

    #[cfg(test)]
    pub(super) fn remaining_connection_receive_window(&self) -> VarInt {
        self.connection_flow_controller.remaining_window()
    }
}

/// The read half of a stream
pub struct ReceiveStream {
    /// The current state of the stream
    pub(super) state: ReceiveStreamState,
    /// Buffer of already received data
    pub(super) receive_buffer: StreamReceiveBuffer,
    /// The composite flow controller for receiving data
    pub(super) flow_controller: ReceiveStreamFlowController,
    /// Synchronizes the `STOP_SENDING` flag towards the peer.
    pub(super) stop_sending_sync: OnceSync<ApplicationErrorCode, StopSendingToFrameWriter>,
    /// The handle of a task that is currently waiting on new incoming data.
    pub(super) read_waiter: Option<Waker>,
    /// Whether the final state had already been observed by the application
    final_state_observed: bool,
}

impl ReceiveStream {
    pub fn new(
        is_closed: bool,
        connection_flow_controller: IncomingConnectionFlowController,
        initial_window: VarInt,
        desired_flow_control_window: u32,
    ) -> ReceiveStream {
        // If the stream is created in closed state directly move into the
        // terminal state.
        let state = if is_closed {
            ReceiveStreamState::DataRead
        } else {
            ReceiveStreamState::Receiving(None)
        };

        let mut result = ReceiveStream {
            state,
            receive_buffer: StreamReceiveBuffer::new(),
            flow_controller: ReceiveStreamFlowController::new(
                connection_flow_controller,
                initial_window,
                desired_flow_control_window,
            ),
            stop_sending_sync: OnceSync::new(),
            read_waiter: None,
            final_state_observed: is_closed,
        };

        if is_closed {
            result.flow_controller.stop_sync();
            result.stop_sending_sync.stop_sync();
        }

        result
    }

    // These functions are called from the packet delivery thread

    pub fn on_data(
        &mut self,
        frame: &StreamRef,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        match self.state {
            ReceiveStreamState::Reset(_) => {
                // Since the stream already had been reset we ignore the data.
                // In this case we don't check for correctness - e.g. whether the
                // would actually have fitted within our flow-control window and
                // into the end-of-stream signal. We could add these checks, but
                // the main outcome would be to send connection errors.
            }
            ReceiveStreamState::DataRead => {
                // We also ignore the data in this case. We could validate whether
                // it actually fitted into previously announced window, but
                // don't get any benefit from this.
            }
            ReceiveStreamState::Receiving(mut total_size) => {
                // In this function errors are returned, but the Stream is left
                // intact. It will be task of the caller to fail the stream
                // with `trigger_internal_reset()`.
                // This decision was made since on a connection error all
                // Streams need to be failed.

                // If the size is known we check against the maximum size.
                // Otherwise we check against the flow control window
                let data_end = frame
                    .offset
                    .checked_add_usize(frame.data.len())
                    .ok_or_else(|| {
                        TransportError::FLOW_CONTROL_ERROR
                            .with_reason("data size overflow")
                            .with_frame_type(frame.tag().into())
                    })?;

                if let Some(total_size) = total_size {
                    if data_end > total_size || frame.is_fin && data_end != total_size {
                        //= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#4.4
                        //# Once a final size for a stream is known, it cannot change.  If a
                        //# RESET_STREAM or STREAM frame is received indicating a change in the
                        //# final size for the stream, an endpoint SHOULD respond with a
                        //# FINAL_SIZE_ERROR error
                        return Err(TransportError::FINAL_SIZE_ERROR
                            .with_reason("Final size changed")
                            .with_frame_type(frame.tag().into()));
                    }
                } else {
                    self.flow_controller
                        .acquire_window_up_to(data_end, frame.tag().into())?;
                }

                let was_empty = self.receive_buffer.is_empty();
                let mut was_closed = false;
                self.receive_buffer
                    .write_at(frame.offset, frame.data)
                    .map_err(|error| {
                        match error {
                            //=https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#19.9
                            //# An endpoint MUST terminate a connection with a FLOW_CONTROL_ERROR
                            //# error if it receives more data than the maximum data value that it
                            //# has sent, unless this is a result of a change in the initial limits
                            //# (see Section 7.3.1).
                            StreamReceiveBufferError::OutOfRange => {
                                TransportError::FLOW_CONTROL_ERROR
                            }
                            StreamReceiveBufferError::AllocationError => {
                                TransportError::INTERNAL_ERROR
                            }
                        }
                        .with_reason("data reception error")
                        .with_frame_type(frame.tag().into())
                    })?;

                if frame.is_fin && total_size.is_none() {
                    // Store the total size
                    total_size = Some(data_end.into());
                    self.state = ReceiveStreamState::Receiving(total_size);
                    // We don't have to transmit MAX_STREAM_DATA frames anymore.
                    // If there is pending transmission/retransmission then remove it.
                    //
                    // This has a subtle side effect that the message which signaled
                    // the higher flow control window might actually have never been
                    // received by the peer (it's pending), and it still was able to send
                    // the FIN and more data to us. Since we neither can prove the peer
                    // right there is nothing we can do about this.

                    //=https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#3.2
                    //# When a STREAM frame with a FIN bit is received, the final size of the
                    //# stream is known (see Section 4.4).  The receiving part of the stream
                    //# then enters the "Size Known" state.  In this state, the endpoint no
                    //# longer needs to send MAX_STREAM_DATA frames, it only receives any
                    //# retransmissions of stream data.
                    self.flow_controller.stop_sync();
                }

                if let Some(total_size) = total_size {
                    // If we already have received all the data, there is no point
                    // in transmitting STOP_SENDING anymore.
                    // Note that this might not hapen in the same frame where we
                    // receive the FIN. We might receive the FIN before receiving
                    // outstanding data.
                    if self.receive_buffer.total_received_len() == total_size {
                        self.stop_sending_sync.stop_sync();
                    }

                    // If the frame with the FIN contained no new data all
                    // buffered data might already have been consumed. In this
                    // case we directly go into [`ReceiveStreamState::DataRead`]
                    if frame.is_fin && self.receive_buffer.consumed_len() == total_size {
                        self.receive_buffer.reset();
                        self.state = ReceiveStreamState::DataRead;

                        // Wakes up any readers that might currently be blocked on
                        // the stream.
                        // If there was no waiter we won't wake up anything. Therefore
                        // this will not lead to an unnecessary wakeup in the case there
                        // is still oustanding buffered data.
                        was_closed = true;
                    }
                }

                if was_empty && (was_closed || !self.receive_buffer.is_empty()) {
                    if let Some(waker) = self.read_waiter.take() {
                        events.store_read_waker(waker);
                    }
                }
            }
        }

        Ok(())
    }

    /// This is called when a `STREAM_DATA_BLOCKED` frame had been received for
    /// this stream
    pub fn on_stream_data_blocked(
        &mut self,
        _frame: &StreamDataBlocked,
        _events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        // There is currently no special handling implemented for this event.
        // In the future we might e.g. generate metrics for this.
        Ok(())
    }

    /// This method gets called when a stream gets reset due to a reason that is
    /// not related to a frame. E.g. due to a connection failure.
    pub fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents) {
        let reset_result = self.init_reset(error, None, None, events);
        // Internal results should never fail
        debug_assert!(reset_result.is_ok());
    }

    /// This is called when a `RESET_STREAM` frame had been received for
    /// this stream
    pub fn on_reset(
        &mut self,
        frame: &ResetStream,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        let error = StreamError::StreamReset(frame.application_error_code.into());
        self.init_reset(error, Some(frame.final_size), Some(frame.tag()), events)
    }

    /// Starts the reset procedure if the Stream has not been in a RESET state
    /// before.
    fn init_reset(
        &mut self,
        error: StreamError,
        actual_size: Option<VarInt>,
        frame_tag: Option<u8>,
        events: &mut StreamEvents,
    ) -> Result<(), TransportError> {
        // Reset logic is only executed if the stream is neither reset nor if all
        // data had been already received.
        match self.state {
            ReceiveStreamState::Reset(_) | ReceiveStreamState::DataRead => return Ok(()),
            ReceiveStreamState::Receiving(Some(total_size)) => {
                if let Some(actual_size) = actual_size {
                    // If the stream size which is indicated through the reset
                    // diverges from the stream size which had been communicated
                    // before this is an error

                    //=https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#4.4
                    //# Once a final size for a stream is known, it cannot change.  If a
                    //# RESET_STREAM or STREAM frame is received indicating a change in the
                    //# final size for the stream, an endpoint SHOULD respond with a
                    //# FINAL_SIZE_ERROR error (see Section 11).  A receiver SHOULD treat
                    //# receipt of data at or beyond the final size as a FINAL_SIZE_ERROR
                    //# error, even after a stream is closed.
                    if Into::<u64>::into(actual_size) != total_size {
                        return Err(TransportError::FINAL_SIZE_ERROR
                            .with_reason(
                                "Final size in reset frame did not match previous final size",
                            )
                            .with_optional_frame_type(frame_tag.map(|tag| tag.into())));
                    }
                }

                if self.receive_buffer.total_received_len() == total_size {
                    // This equals the DataRecvd state from the specification.
                    // We have received all data up to offset total_size and are
                    // just waiting for the user to read it.
                    // In this case we ignore the reset, since we don't require
                    // any information from the peer anymore.
                    return Ok(());
                }
            }
            ReceiveStreamState::Receiving(None) => {
                if let Some(actual_size) = actual_size {
                    // We have to acquire the flow control credits up up to the
                    // offset which the peer indicates as the end of the Stream.
                    // This is necessary since the peer will have reserved credits
                    // up to this offset, and we need to send the necessary
                    // flow control updates.
                    self.flow_controller
                        .acquire_window_up_to(actual_size, frame_tag)?;
                }
            }
        }

        // If the stream was reset by the peer we don't actually have to retransmit
        // outgoing flow control window anymore.
        self.flow_controller.stop_sync();
        // We also don't have to send `STOP_SENDING` anymore
        self.stop_sending_sync.stop_sync();

        // Reset the stream receive buffer
        self.receive_buffer.reset();

        // The data which was inside the receive buffer had actually not been
        // consumed. And if the peer signaled us a bigger final size than what
        // we actually received, we might not even had received the data yet for
        // which we acquired a connection flow control window. Nevertheless we
        // need to release the complete window in order not to starve other
        // streams on connection flow control credits. This is performed by the
        // the following call, which releases ALL credits which have not been
        // previously released.
        self.flow_controller.release_outstanding_window();

        self.state = ReceiveStreamState::Reset(error);

        // Return the waker to wake up potential users of the stream
        if let Some(waker) = self.read_waiter.take() {
            events.store_read_waker(waker);
        }

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        self.flow_controller.read_window_sync.on_packet_ack(ack_set);

        self.stop_sending_sync.on_packet_ack(ack_set);
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        self.flow_controller
            .read_window_sync
            .on_packet_loss(ack_set);

        self.stop_sending_sync.on_packet_loss(ack_set);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.stop_sending_sync.on_transmit(stream_id, context)?;

        self.flow_controller
            .read_window_sync
            .on_transmit(stream_id, context)
    }

    // These functions are called from the client API

    pub fn poll_pop<C: ConnectionContext>(
        &mut self,
        _connection_context: &C,
        context: &Context,
    ) -> Poll<Result<Option<Bytes>, StreamError>> {
        // Do some state checks here. Only read data when the client is still
        // allowed to read (not reset).

        match self.state {
            ReceiveStreamState::Reset(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                Poll::Ready(Err(error))
            }
            ReceiveStreamState::DataRead => {
                // All stream data had been received and all buffered data was
                // already consumed
                self.final_state_observed = true;
                Poll::Ready(Ok(None))
            }
            ReceiveStreamState::Receiving(total_size) => {
                match self.receive_buffer.pop() {
                    Some(data) => {
                        // Release the flow control window for the consumed chunk
                        self.flow_controller
                            .release_window(VarInt::new(data.len() as u64).unwrap());

                        // Check for the end of stream and transition to
                        // [`ReceiveStreamState::DataRead`] if necessary.
                        match total_size {
                            Some(total_size)
                                if total_size == self.receive_buffer.consumed_len() =>
                            {
                                // By the time we enter the final state all synchronization
                                // should have been cancelled.
                                debug_assert!(self.stop_sending_sync.is_cancelled());
                                debug_assert!(self.flow_controller.read_window_sync.is_cancelled());
                                // The client has consumed all data. The stream
                                // is thereby finished.
                                self.state = ReceiveStreamState::DataRead;
                                // We clear the receive buffer, to free up any buffer
                                // space which had been allocated but not used
                                self.receive_buffer.reset();
                            }
                            _ => {
                                // We either don't know the total size yet, or
                                // there is still outstanding data to read
                            }
                        }

                        Poll::Ready(Ok(Some(data.freeze())))
                    }
                    None => {
                        // Store the waker, in order to be able to wakeup the client when
                        // data arrives later.
                        self.read_waiter = Some(context.waker().clone());
                        Poll::Pending
                    }
                }
            }
        }
    }

    pub fn stop_sending<C: ConnectionContext>(
        &mut self,
        error_code: ApplicationErrorCode,
        _connection_context: &C,
    ) -> Result<(), StreamError> {
        // If `STOP_SENDING` had already been requested before, then there is
        // nothing to do. If the `Stream` is already reset or all data had been
        // retrieved then we also don't need to do anything. This happens
        // automatically, since we cancelled delivery of `STOP_SENDING` for those
        // Otherwise request delivery.
        // This logic is all handled by [`OnceSync`]
        self.stop_sending_sync.request_delivery(error_code);

        Ok(())
    }
}

impl StreamInterestProvider for ReceiveStream {
    fn interests(&self) -> StreamInterests {
        let frame_exchange_interests = self.stop_sending_sync.frame_exchange_interests()
            + self
                .flow_controller
                .read_window_sync
                .frame_exchange_interests();

        StreamInterests {
            connection_flow_control_credits: false,
            finalization: self.final_state_observed,
            frame_exchange: frame_exchange_interests,
        }
    }
}
