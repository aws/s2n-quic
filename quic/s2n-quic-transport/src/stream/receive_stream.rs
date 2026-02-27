// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    stream::{
        incoming_connection_flow_controller::IncomingConnectionFlowController,
        stream_events::StreamEvents,
        stream_interests::{StreamInterestProvider, StreamInterests},
        StreamError,
    },
    sync::{IncrementalValueSync, OnceSync, ValueToFrameWriter},
    transmission::interest::Provider as _,
};
use core::{
    convert::TryFrom,
    task::{Context, Poll, Waker},
};
use s2n_quic_core::{
    ack, application,
    buffer::{self, Reassembler},
    frame::{stream::StreamRef, MaxStreamData, ResetStream, StopSending, StreamDataBlocked},
    packet::number::PacketNumber,
    stream::{ops, StreamId},
    transport,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-3.2
//#          o
//#          | Recv STREAM / STREAM_DATA_BLOCKED / RESET_STREAM
//#          | Create Bidirectional Stream (Sending)
//#          | Recv MAX_STREAM_DATA / STOP_SENDING (Bidirectional)
//#          | Create Higher-Numbered Stream
//#          v
//#      +-------+
//#      | Recv  | Recv RESET_STREAM
//#      |       |-----------------------.
//#      +-------+                       |
//#          |                           |
//#          | Recv STREAM + FIN         |
//#          v                           |
//#      +-------+                       |
//#      | Size  | Recv RESET_STREAM     |
//#      | Known |---------------------->|
//#      +-------+                       |
//#          |                           |
//#          | Recv All Data             |
//#          v                           v
//#      +-------+ Recv RESET_STREAM +-------+
//#      | Data  |--- (optional) --->| Reset |
//#      | Recvd |  Recv All Data    | Recvd |
//#      +-------+<-- (optional) ----+-------+
//#          |                           |
//#          | App Read All Data         | App Read Reset
//#          v                           v
//#      +-------+                   +-------+
//#      | Data  |                   | Reset |
//#      | Read  |                   | Read  |
//#      +-------+                   +-------+

/// Enumerates the possible states of the receiving side of a stream.
/// These states are equivalent to the ones in the QUIC transport specification.
#[derive(PartialEq, Debug, Clone)]
pub(super) enum ReceiveStreamState {
    /// The stream is still receiving data from the remote peer. This state
    /// coverst the `Recv`, `Size Known` and `Data Recvd` state from the QUIC
    /// specification. These are modelled as a single state because the handling
    /// for the states is mostly identical.
    /// The parameter indicates the total size of the stream if it had already
    /// been signalled by the peer.
    Receiving,
    /// All data had been received from the peer and consumed by the user.
    /// This is the terminal state.
    DataRead,
    /// The application has requested the peer to STOP_SENDING and the stream is currently
    /// waiting for an ACK for the STOP_SENDING frame.
    Stopping {
        error: StreamError,
        missing_data: MissingData,
    },
    /// The connection was reset. The flag indicates whether the reset status
    /// had already been observed by the user.
    Reset(StreamError),
}

/// Keeps track of any missing data in the `Stopping` state
#[derive(PartialEq, Debug, Clone)]
pub(super) struct MissingData {
    start: u64,
    end: u64,
}

impl MissingData {
    fn new(start: u64) -> Self {
        Self {
            start,
            end: u64::MAX,
        }
    }

    fn on_data(&mut self, frame: &StreamRef) -> Poll<()> {
        // We could track if we have any pending gaps and continue to send STOP_SENDING but
        // that would require keeping the receive buffer around, which isn't really useful
        // since the application has already closed the stream.
        //
        // Instead, we just use a simple range

        let frame_start = *frame.offset;
        let frame_end = *(frame.offset + frame.data.len());
        let frame_range = frame_start..frame_end;

        // update the start if it overlaps the offset of the frame
        if frame_range.contains(&self.start) {
            self.start = frame_end;
        }

        // update the end if this is the last frame or if it contains the current end
        if frame.is_fin || frame_range.contains(&self.end) {
            self.end = self.end.min(frame_start);
        }

        // return if we've received everything
        if self.start >= self.end {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Writes the `MAX_STREAM_DATA` frames based on the streams flow control window.
#[derive(Debug, Default)]
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
#[derive(Debug, Default)]
pub(super) struct StopSendingToFrameWriter {}

impl ValueToFrameWriter<application::Error> for StopSendingToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: application::Error,
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
#[derive(Debug)]
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
    /// limit from the connections flow controller.
    fn acquire_window_up_to(
        &mut self,
        offset: VarInt,
        source_frame_type: Option<u8>,
    ) -> Result<(), transport::Error> {
        // Step 1: Check the stream limit

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.10
        //# The data sent on a stream MUST NOT exceed the largest maximum stream
        //# data value advertised by the receiver.  An endpoint MUST terminate a
        //# connection with an error of type FLOW_CONTROL_ERROR if it receives
        //# more data than the largest maximum stream data that it has sent for
        //# the affected stream.  This includes violations of remembered limits
        //# in Early Data; see Section 7.4.1.
        if offset > self.read_window_sync.latest_value() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
            //# A receiver MUST close the connection with an error of type
            //# FLOW_CONTROL_ERROR if the sender violates the advertised connection
            //# or stream data limits; see Section 11 for details on error handling.

            return Err(transport::Error::FLOW_CONTROL_ERROR
                .with_reason("Stream flow control window exceeded")
                .with_frame_type(source_frame_type.unwrap_or_default().into()));
        }
        // Remark: Actually this read window might not have yet been
        // transmitted to the peer. In that case it might have now
        // successfully sent us data even though we didn't request
        // yet. However even if we knew we sent the MAX_STREAM_DATA frame
        // we wouldn't knew whether the peer actually received it and
        // send their data because of that. Therefore there exists
        // always some uncertainty around the window. The most
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
                .map_err(|err| {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
                    //# A receiver MUST close the connection with an error of type
                    //# FLOW_CONTROL_ERROR if the sender violates the advertised connection
                    //# or stream data limits; see Section 11 for details on error handling.
                    err.with_reason("Connection flow control window exceeded")
                        .with_frame_type(source_frame_type.unwrap_or_default().into())
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.2
        //# Therefore, a receiver MUST NOT wait for a
        //# STREAM_DATA_BLOCKED or DATA_BLOCKED frame before sending a
        //# MAX_STREAM_DATA or MAX_DATA frame; doing so could result in the
        //# sender being blocked for the rest of the connection.

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

    /// Returns the low watermark for the current state of the flow controller
    fn watermark(&self) -> usize {
        // As we approach the flow controller window we want to wake the waiter a bit early
        // to ensure the application has enough time to read the data and release
        // additional credits for the peer to send more data. 50% may need to be
        // modified as additional test are performed. It also may be a good idea to make this
        // configurable in the future.

        // TODO possibly make this value configurable
        let watermark = self.desired_flow_control_window / 2;

        usize::try_from(watermark).unwrap_or(usize::MAX)
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
#[derive(Debug)]
pub struct ReceiveStream {
    /// The current state of the stream
    pub(super) state: ReceiveStreamState,
    /// Buffer of already received data
    pub(super) receive_buffer: Reassembler,
    /// The composite flow controller for receiving data
    pub(super) flow_controller: ReceiveStreamFlowController,
    /// Synchronizes the `STOP_SENDING` flag towards the peer.
    pub(super) stop_sending_sync: OnceSync<application::Error, StopSendingToFrameWriter>,
    /// The handle of a task that is currently waiting on new incoming data, along with the low
    /// watermark value.
    pub(super) read_waiter: Option<(Waker, usize)>,
    /// Whether the final state had already been observed by the application
    final_state_observed: bool,
    /// Marks the stream as detached from the application
    detached: bool,
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
            ReceiveStreamState::Receiving
        };

        let mut result = ReceiveStream {
            state,
            receive_buffer: Reassembler::new(),
            flow_controller: ReceiveStreamFlowController::new(
                connection_flow_controller,
                initial_window,
                desired_flow_control_window,
            ),
            stop_sending_sync: OnceSync::new(),
            read_waiter: None,
            final_state_observed: is_closed,
            detached: is_closed,
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
    ) -> Result<(), transport::Error> {
        match self.state {
            ReceiveStreamState::Reset(_) => {
                // Since the stream already had been reset we ignore the data.
                // In this case we don't check for correctness - e.g. whether the
                // would actually have fitted within our flow-control window and
                // into the end-of-stream signal. We could add these checks, but
                // the main outcome would be to send connection errors.
            }
            ReceiveStreamState::Stopping {
                ref mut missing_data,
                ..
            } => {
                if missing_data.on_data(frame).is_ready() {
                    self.stop_sending_sync.stop_sync();
                    self.final_state_observed = true;
                }
            }
            ReceiveStreamState::DataRead => {
                // We also ignore the data in this case. We could validate whether
                // it actually fitted into previously announced window, but
                // don't get any benefit from this.
            }
            ReceiveStreamState::Receiving => {
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
                        transport::Error::FLOW_CONTROL_ERROR
                            .with_reason("data size overflow")
                            .with_frame_type(frame.tag().into())
                    })?;

                // If we don't know the final size then try acquiring flow control
                //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
                //# The receiver MUST use the final size of the stream to
                //# account for all bytes sent on the stream in its connection level flow
                //# controller.
                if self.receive_buffer.final_size().is_none() {
                    self.flow_controller
                        .acquire_window_up_to(data_end, frame.tag().into())?;
                }

                // If this is the last frame then inform the receive_buffer so it can check for any
                // final size errors.
                let write_result: Result<(), buffer::Error> = if frame.is_fin {
                    self.receive_buffer.write_at_fin(frame.offset, frame.data)
                } else {
                    self.receive_buffer.write_at(frame.offset, frame.data)
                };

                write_result.map_err(|error| {
                    match error {
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.9
                        //# An endpoint MUST terminate a connection with an error of type
                        //# FLOW_CONTROL_ERROR if it receives more data than the maximum data
                        //# value that it has sent.  This includes violations of remembered
                        //# limits in Early Data; see Section 7.4.1.
                        buffer::Error::OutOfRange => transport::Error::FLOW_CONTROL_ERROR,
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
                        //# Once a final size for a stream is known, it cannot change.  If a
                        //# RESET_STREAM or STREAM frame is received indicating a change in the
                        //# final size for the stream, an endpoint SHOULD respond with an error
                        //# of type FINAL_SIZE_ERROR; see Section 11 for details on error
                        //# handling.
                        buffer::Error::InvalidFin => transport::Error::FINAL_SIZE_ERROR,
                        buffer::Error::ReaderError(_) => {
                            unreachable!("reader is infallible")
                        }
                    }
                    .with_reason("data reception error")
                    .with_frame_type(frame.tag().into())
                })?;

                // wake the waiter if the buffer has data and the len has crossed the watermark
                let mut should_wake = self
                    .read_waiter
                    .as_ref()
                    .map(|(_, low_watermark)| {
                        let len = self.receive_buffer.len();

                        // make sure we have at least 1 byte available for reading
                        if len == 0 {
                            return false;
                        }

                        let watermark = (*low_watermark)
                            // don't let the application-provided watermark exceed the flow
                            // controller watermark
                            .min(self.flow_controller.watermark());

                        // ensure the buffer has at least the watermark
                        len >= watermark
                    })
                    .unwrap_or(false);

                if frame.is_fin {
                    // We don't have to transmit MAX_STREAM_DATA frames anymore.
                    // If there is pending transmission/retransmission then remove it.
                    //
                    // This has a subtle side effect that the message which signaled
                    // the higher flow control window might actually have never been
                    // received by the peer (it's pending), and it still was able to send
                    // the FIN and more data to us. Since we neither can prove the peer
                    // right there is nothing we can do about this.

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-3.2
                    //# When a STREAM frame with a FIN bit is received, the final size of the
                    //# stream is known; see Section 4.5.  The receiving part of the stream
                    //# then enters the "Size Known" state.  In this state, the endpoint no
                    //# longer needs to send MAX_STREAM_DATA frames, it only receives any
                    //# retransmissions of stream data.
                    self.flow_controller.stop_sync();
                }

                if let Some(total_size) = self.receive_buffer.final_size() {
                    // If we already have received all the data, there is no point
                    // in transmitting STOP_SENDING anymore.
                    // Note that this might not happen in the same frame where we
                    // receive the FIN. We might receive the FIN before receiving
                    // outstanding data.
                    if self.receive_buffer.total_received_len() == total_size {
                        self.stop_sending_sync.stop_sync();

                        // wake the waiter, even if we didn't cross the watermark, since the stream
                        // is finished at this point
                        should_wake = true;
                    }

                    // If the frame with the FIN contained no new data all
                    // buffered data might already have been consumed. In this
                    // case we directly go into [`ReceiveStreamState::DataRead`]
                    if frame.is_fin && self.receive_buffer.consumed_len() == total_size {
                        self.receive_buffer.reset();
                        self.state = ReceiveStreamState::DataRead;
                    }
                }

                if should_wake {
                    self.wake(events);
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
    ) -> Result<(), transport::Error> {
        // There is currently no special handling implemented for this event.
        // In the future we might e.g. generate metrics for this.
        Ok(())
    }

    /// This method gets called when a stream gets reset due to a reason that is
    /// not related to a frame. E.g. due to a connection failure.
    pub fn on_internal_reset(&mut self, error: StreamError, events: &mut StreamEvents) {
        let reset_result = self.init_reset(error, None, None);
        // Internal results should never fail
        debug_assert!(reset_result.is_ok());

        // Return the waker to wake up potential users of the stream
        self.wake(events);
    }

    /// This is called when a `RESET_STREAM` frame had been received for
    /// this stream
    pub fn on_reset(
        &mut self,
        frame: &ResetStream,
        events: &mut StreamEvents,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.5
        //= type=exception
        //= reason=It's simpler to accept any RESET_STREAM frame instead of ignore
        //# An endpoint that sends a STOP_SENDING frame MAY ignore the
        //# error code in any RESET_STREAM frames subsequently received for that
        //# stream.

        let error = StreamError::stream_reset(frame.application_error_code.into());
        self.init_reset(error, Some(frame.final_size), Some(frame.tag()))?;

        // We don't have to send `STOP_SENDING` anymore since the stream was reset by the peer
        self.stop_sending_sync.stop_sync();

        // Return the waker to wake up potential users of the stream
        self.wake(events);

        Ok(())
    }

    /// Starts the reset procedure if the Stream has not been in a RESET state
    /// before.
    fn init_reset(
        &mut self,
        error: StreamError,
        actual_size: Option<VarInt>,
        frame_tag: Option<u8>,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-3.2
        //# An implementation MAY
        //# interrupt delivery of stream data, discard any data that was not
        //# consumed, and signal the receipt of the RESET_STREAM.

        // Reset logic is only executed if the stream is neither reset nor if all
        // data had been already received.
        match self.state {
            ReceiveStreamState::Reset(_) | ReceiveStreamState::DataRead => return Ok(()),
            ReceiveStreamState::Receiving if self.receive_buffer.final_size().is_some() => {
                let total_size = self.receive_buffer.final_size().unwrap();
                if let Some(actual_size) = actual_size {
                    // If the stream size which is indicated through the reset
                    // diverges from the stream size which had been communicated
                    // before this is an error

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-4.5
                    //# Once a final size for a stream is known, it cannot change.  If a
                    //# RESET_STREAM or STREAM frame is received indicating a change in the
                    //# final size for the stream, an endpoint SHOULD respond with an error
                    //# of type FINAL_SIZE_ERROR; see Section 11 for details on error
                    //# handling.  A receiver SHOULD treat receipt of data at or beyond the
                    //# final size as an error of type FINAL_SIZE_ERROR, even after a stream
                    //# is closed.
                    if Into::<u64>::into(actual_size) != total_size {
                        return Err(transport::Error::FINAL_SIZE_ERROR
                            .with_reason(
                                "Final size in reset frame did not match previous final size",
                            )
                            .with_frame_type(frame_tag.unwrap_or_default().into()));
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
            ReceiveStreamState::Receiving | ReceiveStreamState::Stopping { .. } => {
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

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.flow_controller.read_window_sync.on_packet_ack(ack_set);
        let _ = self.stop_sending_sync.on_packet_ack(ack_set);
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.2
        //= type=TODO
        //= tracking-issue=334
        //# To avoid blocking a sender, a receiver MAY send a MAX_STREAM_DATA or
        //# MAX_DATA frame multiple times within a round trip or send it early
        //# enough to allow time for loss of the frame and subsequent recovery.
        self.flow_controller
            .read_window_sync
            .on_transmit(stream_id, context)
    }

    /// Wakes up the application on progress updates
    ///
    /// If there is not a registered waker and the stream is in a terminal state,
    /// the stream will be finalized.
    fn wake(&mut self, events: &mut StreamEvents) {
        // Return the waker to wake up potential users of the stream
        if let Some((waker, _low_watermark)) = self.read_waiter.take() {
            events.store_read_waker(waker);
            return;
        }

        // If the stream is detached from the application, then try to make progress
        if self.detached {
            self.detach();
        }
    }

    // These functions are called from the client API

    pub fn poll_request(
        &mut self,
        request: &mut ops::rx::Request,
        context: Option<&Context>,
    ) -> Result<ops::rx::Response, StreamError> {
        let mut response = ops::rx::Response::default();

        if let Some(error_code) = request.stop_sending {
            let error = StreamError::stream_reset(error_code);

            match self.state {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-3.3
                //# A receiver MAY send a STOP_SENDING frame in any state where it has
                //# not received a RESET_STREAM frame -- that is, states other than
                //# "Reset Recvd" or "Reset Read".

                //= https://www.rfc-editor.org/rfc/rfc9000#section-3.5
                //# STOP_SENDING SHOULD only be sent for a stream that has not been reset
                //# by the peer.
                ReceiveStreamState::Reset(error) | ReceiveStreamState::Stopping { error, .. } => {
                    response.status = ops::Status::Reset(error);
                    return Ok(response);
                }
                // If we've already read everything, transition to the final state
                ReceiveStreamState::DataRead => {
                    self.state = ReceiveStreamState::DataRead;
                    self.final_state_observed = true;
                    response.status = ops::Status::Finished;
                    return Ok(response);
                }
                // If we've already buffered everything, transition to the final state
                ReceiveStreamState::Receiving if self.receive_buffer.is_writing_complete() => {
                    self.state = ReceiveStreamState::DataRead;
                    self.final_state_observed = true;
                    response.status = ops::Status::Finished;
                    return Ok(response);
                }
                //= https://www.rfc-editor.org/rfc/rfc9000#section-3.5
                //# If the stream is in the "Recv" or "Size Known" states, the transport
                //# SHOULD signal this by sending a STOP_SENDING frame to prompt closure
                //# of the stream in the opposite direction.
                _ => {
                    self.stop_sending_sync.request_delivery(error_code);

                    let received_len = self.receive_buffer.total_received_len();
                    let missing_data = MissingData::new(received_len);
                    // transition to the Stopping state so we can start shutting down
                    self.state = ReceiveStreamState::Stopping {
                        error,
                        missing_data,
                    };
                }
            }

            // STOP_SENDING cannot be flushed so it naturally operates in detached mode
            self.detach();

            // We clear the receive buffer, to free up any buffer
            // space which had been allocated but not used
            self.receive_buffer.reset();

            // Mark the stream as reset. Note that the request doesn't have a flush so there's
            // currently no way to wait for the reset to be acknowledged.
            response.status = ops::Status::Reset(error);

            return Ok(response);
        }

        if request.detached {
            self.detach();
        }

        // Do some state checks here. Only read data when the client is still
        // allowed to read (not reset).

        let total_size = match self.state {
            ReceiveStreamState::Reset(error) => {
                // The reset is now known to have been read by the client.
                self.final_state_observed = true;
                self.read_waiter = None;
                return Err(error);
            }
            ReceiveStreamState::Stopping { error, .. } => {
                self.read_waiter = None;
                return Err(error);
            }
            ReceiveStreamState::DataRead => {
                // All stream data had been received and all buffered data was
                // already consumed
                self.final_state_observed = true;
                self.read_waiter = None;
                response.status = ops::Status::Finished;
                return Ok(response);
            }
            ReceiveStreamState::Receiving => self.receive_buffer.final_size(),
        };

        let low_watermark = &mut request.low_watermark;
        let high_watermark = &mut request.high_watermark;
        let mut should_wake = false;

        // ensure the number of available bytes is at least the requested low watermark
        if self.receive_buffer.len() >= self.flow_controller.watermark().min(*low_watermark) {
            if let Some(chunks) = request.chunks.as_mut().filter(|chunks| !chunks.is_empty()) {
                // Make sure all of the placeholder chunks are empty. If it's not, it could lead to
                // replacing a chunk that was received in a previous request.
                //
                // We iterate over all of the chunks to make sure we don't do a partial write and
                // return an error (which would result in losing data).
                if chunks.iter().any(|chunk| !chunk.is_empty()) {
                    return Err(StreamError::non_empty_output());
                }

                while response.chunks.consumed < chunks.len() {
                    if let Some(data) = self.receive_buffer.pop_watermarked(*high_watermark) {
                        let data_len = data.len();
                        // Release the flow control window for the consumed chunk
                        self.flow_controller.release_window(
                            VarInt::try_from(data_len)
                                .expect("chunk len should always be less than maximum VarInt"),
                        );
                        *low_watermark = (*low_watermark).saturating_sub(data_len);
                        *high_watermark = (*high_watermark).saturating_sub(data_len);

                        // replace the placeholder with the actual data
                        let placeholder = core::mem::replace(
                            &mut chunks[response.chunks.consumed],
                            data.freeze(),
                        );
                        debug_assert!(
                            placeholder.is_empty(),
                            "the placeholder should never contain data"
                        );

                        response.bytes.consumed += data_len;
                        response.chunks.consumed += 1;
                    } else {
                        // wake the request if we didn't consume anything
                        should_wake |= response.chunks.consumed == 0;
                        break;
                    }
                }
            }
        } else {
            // notify when we have at least the requested watermark
            should_wake = true;
        }

        // Check for the end of stream and transition to
        // [`ReceiveStreamState::DataRead`] if necessary.
        if let Some(total_size) = total_size {
            if total_size == self.receive_buffer.consumed_len() {
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

                // clear the waiter
                self.read_waiter = None;

                // mark the final state as observed - the caller is expected to cache the `Finished` status
                self.final_state_observed = true;

                // Indicate that all data has been read
                response.status = ops::Status::Finished;
            } else if total_size == self.receive_buffer.total_received_len() {
                // inform callers that the stream will not increase beyond its current size
                response.status = ops::Status::Finishing;
            }
        }

        let (available_bytes, available_chunks) = self.receive_buffer.report();
        response.bytes.available = available_bytes;
        response.chunks.available = available_chunks;

        if should_wake {
            if let Some(context) = context {
                // Store the waker, in order to be able to wakeup the client when
                // data arrives later.
                self.read_waiter = Some((context.waker().clone(), request.low_watermark));
                response.will_wake = true;
            }
        }

        Ok(response)
    }

    fn detach(&mut self) {
        debug_assert!(
            matches!(
                &self.state,
                ReceiveStreamState::DataRead
                    | ReceiveStreamState::Reset(_)
                    | ReceiveStreamState::Stopping { .. }
            ),
            "a receive stream should only detach in a finalizing state"
        );

        self.detached = true;
        self.read_waiter = None;

        match &self.state {
            // if the application has read the entire stream, then we can finalize the stream
            ReceiveStreamState::DataRead => {
                self.final_state_observed = true;
            }
            // if the stream has been reset and the application isn't subscribed to updates
            ReceiveStreamState::Reset(_) => {
                self.final_state_observed = true;
            }
            _ => {}
        }
    }
}

impl StreamInterestProvider for ReceiveStream {
    #[inline]
    fn stream_interests(&self, interests: &mut StreamInterests) {
        if self.final_state_observed {
            return;
        }

        // let the stream container know we still have work to do
        interests.retained = true;

        interests.delivery_notifications |= self.stop_sending_sync.is_inflight()
            || self.flow_controller.read_window_sync.is_inflight();

        interests.with_transmission(|query| {
            self.stop_sending_sync.transmission_interest(query)?;
            self.flow_controller
                .read_window_sync
                .transmission_interest(query)?;
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests;
