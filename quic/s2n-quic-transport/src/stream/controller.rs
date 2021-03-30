// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::OnTransmitError,
    sync::{IncrementalValueSync, ValueToFrameWriter},
    transmission,
    transmission::{interest::Provider, WriteContext},
};
use core::task::{Context, Poll, Waker};
use s2n_quic_core::{
    ack,
    frame::MaxStreams,
    packet::number::PacketNumber,
    stream,
    stream::{StreamId, StreamType},
    transport,
    transport::parameters::InitialFlowControlLimits,
    varint::VarInt,
};
use smallvec::SmallVec;

/// This component manages stream concurrency limits.
///
/// It enforces both the local initiated stream limits and the peer initiated
/// stream limits.
///
/// It will also signal an increased max streams once streams have been consumed.
#[derive(Debug)]
pub struct Controller {
    uni_controller: ControllerImpl,
    bidi_controller: ControllerImpl,
}

impl Controller {
    /// Creates a new `stream::Controller`
    ///
    /// The peer will be allowed to open streams up to the given `initial_local_limits`.
    ///
    /// The local application will be allowed to open streams up to the minimum of the
    /// given local limits (`initial_local_limits` and `stream_limits`) and `initial_peer_limits`.
    pub fn new(
        initial_peer_limits: InitialFlowControlLimits,
        initial_local_limits: InitialFlowControlLimits,
        stream_limits: stream::Limits,
    ) -> Self {
        Self {
            uni_controller: ControllerImpl::new(
                initial_peer_limits.max_streams_uni,
                // Unidirectional streams may have asymmetric concurrent stream limits since the
                // cost of a send stream is not equal to the cost of receive stream.
                stream_limits.max_open_local_unidirectional_streams,
                initial_local_limits.max_streams_uni,
            ),
            bidi_controller: ControllerImpl::new(
                initial_peer_limits.max_streams_bidi,
                // Bidirectional streams have the same value for local and peer initiated stream
                // limits since data may flow in either direction regardless of which side initiated
                // the stream.
                initial_local_limits.max_streams_bidi,
                initial_local_limits.max_streams_bidi,
            ),
        }
    }

    /// This method is called when a `MAX_STREAMS` frame is received,
    /// which signals an increase in the available streams budget.
    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
        match frame.stream_type {
            StreamType::Bidirectional => self.bidi_controller.on_max_streams(frame),
            StreamType::Unidirectional => self.uni_controller.on_max_streams(frame),
        }
    }

    /// This method is called when the local application wishes to open a new stream.
    ///
    /// `Poll::Pending` is returned when there isn't available capacity to open a stream,
    /// either because of local initiated concurrency limits or the peer's stream limits.
    /// If `Poll::Pending` is returned, the waker in the given `context` will be woken
    /// when additional stream capacity becomes available.
    pub fn poll_local_open_stream(
        &mut self,
        stream_type: StreamType,
        context: &Context,
    ) -> Poll<()> {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.poll_local_open_stream(context),
            StreamType::Unidirectional => self.uni_controller.poll_local_open_stream(context),
        }
    }

    /// This method is called when the remote peer wishes to open a new stream.
    ///
    /// A `STREAM_LIMIT_ERROR` will be returned if the peer has exceeded the stream limits
    /// that were communicated by transport parameters or MAX_STREAMS frames.
    pub fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        match stream_id.stream_type() {
            StreamType::Bidirectional => self.bidi_controller.on_remote_open_stream(stream_id),
            StreamType::Unidirectional => self.uni_controller.on_remote_open_stream(stream_id),
        }
    }

    /// This method is called whenever a stream is opened, regardless of which side initiated.
    pub fn on_open_stream(&mut self, stream_type: StreamType) {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.on_open_stream(),
            StreamType::Unidirectional => self.uni_controller.on_open_stream(),
        }
    }

    /// This method is called whenever a stream is closed.
    pub fn on_close_stream(&mut self, stream_type: StreamType) {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.on_close_stream(),
            StreamType::Unidirectional => self.uni_controller.on_close_stream(),
        }
    }

    /// This method is called when the stream manager is closed. All wakers will be woken
    /// to unblock waiting tasks.
    pub fn close(&mut self) {
        self.bidi_controller.wake_all();
        self.uni_controller.wake_all();
    }

    /// This method is called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller.max_streams_sync.on_packet_ack(ack_set);
        self.uni_controller.max_streams_sync.on_packet_ack(ack_set);
    }

    /// This method is called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller
            .max_streams_sync
            .on_packet_loss(ack_set);
        self.uni_controller.max_streams_sync.on_packet_loss(ack_set);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Only the stream_type from the StreamId is transmitted
        self.bidi_controller.max_streams_sync.on_transmit(
            StreamId::initial(context.local_endpoint_type(), StreamType::Bidirectional),
            context,
        )?;
        self.uni_controller.max_streams_sync.on_transmit(
            StreamId::initial(context.local_endpoint_type(), StreamType::Unidirectional),
            context,
        )
    }

    /// Queries the component for interest in transmitting frames
    pub fn transmission_interest(&self) -> transmission::Interest {
        self.bidi_controller
            .max_streams_sync
            .transmission_interest()
            + self.uni_controller.max_streams_sync.transmission_interest()
    }
}
// The amount of wakers that may be tracked before allocating to the heap.
const WAKERS_INITIAL_CAPACITY: usize = 5;
//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
// Send a MAX_STREAMS frame whenever 1/10th of the window has been closed
pub(super) const MAX_STREAMS_SYNC_FRACTION: VarInt = VarInt::from_u8(10);
//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
//# Maximum Streams:  A count of the cumulative number of streams of the
//# corresponding type that can be opened over the lifetime of the
//# connection.  This value cannot exceed 2^60, as it is not possible
//# to encode stream IDs larger than 2^62-1.
// Safety: 2^60 is less than MAX_VARINT_VALUE
const MAX_STREAMS_MAX_VALUE: VarInt = unsafe { VarInt::new_unchecked(1_152_921_504_606_846_976) };

#[derive(Debug)]
struct ControllerImpl {
    local_initiated_concurrent_stream_limit: VarInt,
    peer_initiated_concurrent_stream_limit: VarInt,
    peer_cumulative_stream_limit: VarInt,
    wakers: SmallVec<[Waker; WAKERS_INITIAL_CAPACITY]>,
    max_streams_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    opened_streams: VarInt,
    closed_streams: VarInt,
}

impl ControllerImpl {
    fn new(
        initial_peer_maximum_streams: VarInt,
        local_initiated_concurrent_stream_limit: VarInt,
        peer_initiated_concurrent_stream_limit: VarInt,
    ) -> Self {
        Self {
            local_initiated_concurrent_stream_limit,
            peer_initiated_concurrent_stream_limit,
            peer_cumulative_stream_limit: initial_peer_maximum_streams,
            wakers: SmallVec::new(),
            max_streams_sync: IncrementalValueSync::new(
                peer_initiated_concurrent_stream_limit,
                peer_initiated_concurrent_stream_limit,
                peer_initiated_concurrent_stream_limit / MAX_STREAMS_SYNC_FRACTION,
            ),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
        }
    }

    fn on_max_streams(&mut self, frame: &MaxStreams) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
        //# A receiver MUST
        //# ignore any MAX_STREAMS frame that does not increase the stream limit.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
        //# MAX_STREAMS frames that do not increase the stream limit MUST be
        //# ignored.
        if self.peer_cumulative_stream_limit >= frame.maximum_streams {
            return;
        }

        self.peer_cumulative_stream_limit = frame.maximum_streams;

        self.wake_unblocked();
    }

    fn poll_local_open_stream(&mut self, context: &Context) -> Poll<()> {
        if self.available_local_stream_capacity() < VarInt::from_u32(1) {
            // Store a waker that can be woken when we get more credit
            self.wakers.push(context.waker().clone());
            return Poll::Pending;
        }
        Poll::Ready(())
    }

    fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        let max_stream_id = StreamId::nth(
            stream_id.initiator(),
            stream_id.stream_type(),
            self.max_streams_sync.latest_value().as_u64(),
        )
        .expect("max_streams is limited to MAX_STREAMS_MAX_VALUE");

        if stream_id > max_stream_id {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
            //# An endpoint
            //# that receives a frame with a stream ID exceeding the limit it has
            //# sent MUST treat this as a connection error of type STREAM_LIMIT_ERROR
            //# (Section 11).

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.11
            //# An endpoint MUST terminate a connection
            //# with a STREAM_LIMIT_ERROR error if a peer opens more streams than was
            //# permitted.
            return Err(transport::Error::STREAM_LIMIT_ERROR);
        }
        Ok(())
    }

    fn on_open_stream(&mut self) {
        self.opened_streams += 1;

        self.check_integrity();
    }

    fn on_close_stream(&mut self) {
        self.closed_streams += 1;

        let max_streams = self
            .closed_streams
            .saturating_add(self.peer_initiated_concurrent_stream_limit)
            .min(MAX_STREAMS_MAX_VALUE);
        self.max_streams_sync.update_latest_value(max_streams);

        self.wake_unblocked();

        self.check_integrity();
    }

    /// The number of streams that may be opened by the local application, respecting both
    /// the local concurrent streams limit and the peer's stream limits.
    fn available_local_stream_capacity(&self) -> VarInt {
        let local_capacity =
            self.local_initiated_concurrent_stream_limit - self.open_stream_count();
        let peer_capacity = self.peer_cumulative_stream_limit - self.opened_streams;
        local_capacity.min(peer_capacity)
    }

    /// Wake all wakers
    fn wake_all(&mut self) {
        self.wakers
            .drain(..self.wakers.len())
            .for_each(|waker| waker.wake())
    }

    /// Wakes the wakers that have been unblocked by the current amount
    /// of available local stream capacity.
    fn wake_unblocked(&mut self) {
        let unblocked_wakers_count = self
            .wakers
            .len()
            .min(self.available_local_stream_capacity().as_u64() as usize);

        self.wakers
            .drain(..unblocked_wakers_count)
            .for_each(|waker| waker.wake());
    }

    /// Returns the number of streams currently open
    fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            assert!(
                self.closed_streams <= self.opened_streams,
                "Cannot close more streams than previously opened"
            );
            assert!(
                self.open_stream_count()
                    <= self
                        .peer_initiated_concurrent_stream_limit
                        .max(self.local_initiated_concurrent_stream_limit),
                "Cannot have more streams open concurrently than
                the larger of the local and peer limits"
            );
        }
    }
}

/// Writes the `MAX_STREAMS` frames based on the stream control window.
#[derive(Debug, Default)]
pub(super) struct MaxStreamsToFrameWriter {}

impl ValueToFrameWriter<VarInt> for MaxStreamsToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&MaxStreams {
            stream_type: stream_id.stream_type(),
            maximum_streams: value,
        })
    }
}

#[cfg(test)]
impl Controller {
    pub fn available_local_stream_capacity(&self, stream_type: StreamType) -> VarInt {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.available_local_stream_capacity(),
            StreamType::Unidirectional => self.uni_controller.available_local_stream_capacity(),
        }
    }

    pub fn max_streams_latest_value(&self, stream_type: StreamType) -> VarInt {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.max_streams_sync.latest_value(),
            StreamType::Unidirectional => self.uni_controller.max_streams_sync.latest_value(),
        }
    }
}
