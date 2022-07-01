// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, open_token},
    contexts::OnTransmitError,
    sync::{IncrementalValueSync, PeriodicSync, ValueToFrameWriter},
    transmission,
    transmission::{interest::Provider, WriteContext},
};
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use s2n_quic_core::{
    ack, endpoint,
    frame::{MaxStreams, StreamsBlocked},
    packet::number::PacketNumber,
    stream,
    stream::{StreamId, StreamType},
    time::{timer, Timestamp},
    transport,
    transport::parameters::InitialFlowControlLimits,
    varint::VarInt,
};
use smallvec::SmallVec;

enum StreamDirection {
    // A stream that both transmits and receives data
    Bidirectional,
    // A stream that transmits data (unidirectional)
    Outgoing,
    // A stream that receives data (unidirectional)
    Incoming,
}

/// This component manages stream concurrency limits.
///
/// It enforces both the local initiated stream limits and the peer initiated
/// stream limits.
///
/// It will also signal an increased max streams once streams have been consumed.
#[derive(Debug)]
pub struct Controller {
    local_endpoint_type: endpoint::Type,
    bidi_controller: ControllerPair,
    uni_controller: ControllerPair,
}

impl Controller {
    /// Creates a new `stream::Controller`
    ///
    /// The peer will be allowed to open streams up to the given `initial_local_limits`.
    ///
    /// For outgoing unidirectional streams, the local application will be allowed to open
    /// up to the minimum of the given local limits (`stream_limits`) and `initial_peer_limits`.
    ///
    /// For bidirectional streams, the local application will be allowed to open
    /// up to the minimum of the given `initial_local_limits` and `initial_peer_limits`.
    ///
    /// The peer may give additional credit to open more streams by delivering `MAX_STREAMS` frames.
    pub fn new(
        local_endpoint_type: endpoint::Type,
        initial_peer_limits: InitialFlowControlLimits,
        initial_local_limits: InitialFlowControlLimits,
        stream_limits: stream::Limits,
    ) -> Self {
        Self {
            local_endpoint_type,
            bidi_controller: ControllerPair {
                stream_id: StreamId::initial(local_endpoint_type, StreamType::Bidirectional),
                outgoing: OutgoingController::new(
                    initial_peer_limits.max_streams_bidi,
                    initial_local_limits.max_streams_bidi,
                ),
                incoming: IncomingController::new(initial_local_limits.max_streams_bidi),
            },
            uni_controller: ControllerPair {
                stream_id: StreamId::initial(local_endpoint_type, StreamType::Unidirectional),
                outgoing: OutgoingController::new(
                    initial_peer_limits.max_streams_uni,
                    stream_limits.max_open_local_unidirectional_streams,
                ),
                incoming: IncomingController::new(initial_local_limits.max_streams_uni),
            },
        }
    }

    /// This method is called when a `MAX_STREAMS` frame is received,
    /// which signals an increase in the available streams budget.
    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
        match frame.stream_type {
            StreamType::Bidirectional => self.bidi_controller.outgoing.on_max_streams(frame),
            StreamType::Unidirectional => self.uni_controller.outgoing.on_max_streams(frame),
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
        open_tokens: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<()> {
        match stream_type {
            StreamType::Bidirectional => self
                .bidi_controller
                .outgoing
                .poll_open_stream(&mut open_tokens.bidirectional, context),
            StreamType::Unidirectional => self
                .uni_controller
                .outgoing
                .poll_open_stream(&mut open_tokens.unidirectional, context),
        }
    }

    /// This method is called whenever a stream is opened, regardless of which side initiated.
    pub fn on_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        match self.direction(stream_id) {
            StreamDirection::Bidirectional => self
                .bidi_controller
                .on_open_stream(stream_id, self.local_endpoint_type),
            StreamDirection::Outgoing => self
                .uni_controller
                .outgoing
                .on_open_stream(stream_id, self.local_endpoint_type),
            StreamDirection::Incoming => self.uni_controller.incoming.on_open_stream(stream_id),
        }
    }

    /// This method is called whenever a stream is closed.
    pub fn on_close_stream(&mut self, stream_id: StreamId) {
        match self.direction(stream_id) {
            StreamDirection::Bidirectional => self.bidi_controller.on_close_stream(),
            StreamDirection::Outgoing => self.uni_controller.outgoing.on_close_stream(),
            StreamDirection::Incoming => self.uni_controller.incoming.on_close_stream(),
        }
    }

    /// This method is called when the stream manager is closed. All wakers will be woken
    /// to unblock waiting tasks.
    pub fn close(&mut self) {
        self.bidi_controller.close();
        self.uni_controller.close();
    }

    /// This method is called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller.on_packet_ack(ack_set);
        self.uni_controller.on_packet_ack(ack_set);
    }

    /// This method is called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller.on_packet_loss(ack_set);
        self.uni_controller.on_packet_loss(ack_set);
    }

    /// Updates the period at which `STREAMS_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    pub fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.bidi_controller
            .outgoing
            .streams_blocked_sync
            .update_sync_period(blocked_sync_period);
        self.uni_controller
            .outgoing
            .streams_blocked_sync
            .update_sync_period(blocked_sync_period);
    }

    /// Queries the component for any outgoing frames that need to get sent
    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        if !self.has_transmission_interest() {
            return Ok(());
        }

        self.bidi_controller.on_transmit(context)?;
        self.uni_controller.on_transmit(context)?;

        Ok(())
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.bidi_controller.on_timeout(now);
        self.uni_controller.on_timeout(now);
    }

    fn direction(&self, stream_id: StreamId) -> StreamDirection {
        match stream_id.stream_type() {
            StreamType::Bidirectional => StreamDirection::Bidirectional,
            StreamType::Unidirectional if stream_id.initiator() == self.local_endpoint_type => {
                StreamDirection::Outgoing
            }
            StreamType::Unidirectional => StreamDirection::Incoming,
        }
    }

    #[cfg(test)]
    // Invoke check when opening peer initiated streams. This check is performed
    // on the IncomingController when opening streams.
    pub fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        match stream_id.stream_type() {
            StreamType::Bidirectional => self
                .bidi_controller
                .incoming
                .on_remote_open_stream(stream_id),
            StreamType::Unidirectional => self
                .uni_controller
                .incoming
                .on_remote_open_stream(stream_id),
        }
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.bidi_controller.timers(query)?;
        self.uni_controller.timers(query)?;
        Ok(())
    }
}

/// Queries the component for interest in transmitting frames
impl transmission::interest::Provider for Controller {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.bidi_controller.transmission_interest(query)?;
        self.uni_controller.transmission_interest(query)?;
        Ok(())
    }
}

/// The controller pair consists of both outgoing and incoming
/// controllers that are both notified when a stream is opened, regardless
/// of which side initiated the stream.
#[derive(Debug)]
struct ControllerPair {
    stream_id: StreamId,
    outgoing: OutgoingController,
    incoming: IncomingController,
}

impl ControllerPair {
    #[inline]
    fn on_open_stream(
        &mut self,
        stream_id: StreamId,
        local_endpoint_type: endpoint::Type,
    ) -> Result<(), transport::Error> {
        self.outgoing
            .on_open_stream(stream_id, local_endpoint_type)?;
        self.incoming.on_open_stream(stream_id)?;
        Ok(())
    }

    #[inline]
    fn on_close_stream(&mut self) {
        self.outgoing.on_close_stream();
        self.incoming.on_close_stream();
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.incoming.on_packet_ack(ack_set);
        self.outgoing.on_packet_ack(ack_set);
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.incoming.on_packet_loss(ack_set);
        self.outgoing.on_packet_loss(ack_set);
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.outgoing.on_timeout(now);
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Only the stream_type from the StreamId is transmitted
        self.incoming.on_transmit(self.stream_id, context)?;
        self.outgoing.on_transmit(self.stream_id, context)
    }

    /// This method is called when the stream manager is closed. All wakers will be woken
    /// to unblock waiting tasks.
    pub fn close(&mut self) {
        self.outgoing.close();
        self.incoming.close();
    }
}

impl timer::Provider for ControllerPair {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.outgoing.timers(query)?;
        Ok(())
    }
}

impl transmission::interest::Provider for ControllerPair {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.incoming.transmission_interest(query)?;
        self.outgoing.transmission_interest(query)?;
        Ok(())
    }
}

// The amount of wakers that may be tracked before allocating to the heap.
const WAKERS_INITIAL_CAPACITY: usize = 5;

/// The OutgoingController controls streams initiated locally
#[derive(Debug)]
struct OutgoingController {
    local_initiated_concurrent_stream_limit: VarInt,
    peer_cumulative_stream_limit: VarInt,
    wakers: SmallVec<[Waker; WAKERS_INITIAL_CAPACITY]>,
    streams_blocked_sync: PeriodicSync<VarInt, StreamsBlockedToFrameWriter>,
    opened_streams: VarInt,
    closed_streams: VarInt,
    /// Keeps track of all of the issued open tokens
    token_counter: open_token::Counter,
    /// Keeps track of all of the expired open tokens
    expired_token: open_token::Token,
}

impl OutgoingController {
    fn new(
        initial_peer_maximum_streams: VarInt,
        local_initiated_concurrent_stream_limit: VarInt,
    ) -> Self {
        Self {
            local_initiated_concurrent_stream_limit,
            peer_cumulative_stream_limit: initial_peer_maximum_streams,
            wakers: SmallVec::new(),
            streams_blocked_sync: PeriodicSync::new(),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
            token_counter: open_token::Counter::new(),
            expired_token: open_token::Token::new(),
        }
    }

    #[inline]
    fn on_max_streams(&mut self, frame: &MaxStreams) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
        //# MAX_STREAMS frames that do not increase the stream limit MUST be ignored.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
        //# MAX_STREAMS frames that do not increase the stream limit MUST be ignored.
        if self.peer_cumulative_stream_limit >= frame.maximum_streams {
            return;
        }

        self.peer_cumulative_stream_limit = frame.maximum_streams;

        // We now have more capacity from the peer so stop sending STREAMS_BLOCKED frames
        self.streams_blocked_sync.stop_sync();

        self.wake_unblocked();
        self.check_integrity();
    }

    #[inline]
    fn poll_open_stream(
        &mut self,
        open_token: &mut open_token::Token,
        context: &Context,
    ) -> Poll<()> {
        if self.available_stream_capacity() < VarInt::from_u32(1) {
            if let Some(index) = open_token.index(&self.expired_token) {
                let prev = &self.wakers[index];
                // update the waker if it's changed
                if !prev.will_wake(context.waker()) {
                    self.wakers[index] = context.waker().clone();
                }
            } else {
                // Store a waker that can be woken when we get more credit
                self.wakers.push(context.waker().clone());
                // give them a waker to remember their position in the list
                *open_token = self.token_counter.next();
            }

            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
            //# An endpoint that is unable to open a new stream due to the peer's
            //# limits SHOULD send a STREAMS_BLOCKED frame (Section 19.14).

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
            //# A sender SHOULD send a STREAMS_BLOCKED frame (type=0x16 or 0x17) when
            //# it wishes to open a stream but is unable to do so due to the maximum
            //# stream limit set by its peer; see Section 19.11.
            if self.peer_capacity() < VarInt::from_u32(1) {
                self.streams_blocked_sync
                    .request_delivery(self.peer_cumulative_stream_limit)
            }

            self.check_integrity();
            return Poll::Pending;
        }

        // reset the open token since they're no longer blocked
        open_token.clear();

        self.check_integrity();
        Poll::Ready(())
    }

    #[inline]
    fn on_local_open_stream(
        &self,
        stream_id: StreamId,
        local_endpoint_type: endpoint::Type,
    ) -> Result<(), transport::Error> {
        // open a total of allowed_limit streams
        let allowed_limit = self.local_initiated_concurrent_stream_limit.as_u64();
        // open maximum of allowed_streams; streams as 0-indexed
        let allowed_streams = allowed_limit
            .checked_sub(1)
            .ok_or(transport::Error::STREAM_LIMIT_ERROR)?;

        let max_stream_id = StreamId::nth(
            stream_id.initiator(),
            stream_id.stream_type(),
            allowed_streams,
        )
        .expect("max_streams is limited to MAX_STREAMS_MAX_VALUE");

        if stream_id > max_stream_id {
            dbg!(
                "{} {} {:?} {:?}",
                allowed_limit,
                allowed_streams,
                max_stream_id,
                stream_id
            );
            if local_endpoint_type == stream_id.initiator() {
                // flow control limits
                return Err(transport::Error::INTERNAL_ERROR);
            } else {
                // peer violated the limits
                //
                //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
                //# Endpoints MUST NOT exceed the limit set by their peer.  An endpoint
                //# that receives a frame with a stream ID exceeding the limit it has
                //# sent MUST treat this as a connection error of type
                //# STREAM_LIMIT_ERROR; see Section 11 for details on error handling.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
                //# An endpoint MUST terminate a connection
                //# with an error of type STREAM_LIMIT_ERROR if a peer opens more streams
                //# than was permitted.
                return Err(transport::Error::STREAM_LIMIT_ERROR);
            }
        }
        Ok(())
    }

    #[inline]
    fn on_open_stream(
        &mut self,
        stream_id: StreamId,
        local_endpoint_type: endpoint::Type,
    ) -> Result<(), transport::Error> {
        self.on_local_open_stream(stream_id, local_endpoint_type)?;
        self.opened_streams += 1;

        self.check_integrity();
        Ok(())
    }

    fn on_close_stream(&mut self) {
        self.closed_streams += 1;

        self.wake_unblocked();
        self.check_integrity();
    }

    /// The number of streams that may be opened by the local application, respecting both
    /// the local concurrent streams limit and the peer's stream limits.
    #[inline]
    fn available_stream_capacity(&self) -> VarInt {
        let local_capacity = self
            .local_initiated_concurrent_stream_limit
            .saturating_sub(self.open_stream_count());
        local_capacity.min(self.peer_capacity())
    }

    /// The current number of streams that can be opened according to the peer's limits
    #[inline]
    fn peer_capacity(&self) -> VarInt {
        self.peer_cumulative_stream_limit
            .saturating_sub(self.opened_streams)
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
            .min(self.available_stream_capacity().as_u64() as usize);

        self.wakers
            .drain(..unblocked_wakers_count)
            .for_each(|waker| waker.wake());

        // keep track of the number of tokens that have expired
        self.expired_token.expire(unblocked_wakers_count);
    }

    /// Returns the number of streams currently open
    fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.streams_blocked_sync.on_timeout(now);
        self.check_integrity();
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.streams_blocked_sync.on_packet_ack(ack_set);
        self.check_integrity();
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.streams_blocked_sync.on_packet_loss(ack_set);
        self.check_integrity();
    }

    #[inline]
    fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        if context.ack_elicitation().is_ack_eliciting() && self.streams_blocked_sync.has_delivered()
        {
            // We are already sending an ack-eliciting packet, so no need to send another STREAMS_BLOCKED.
            // This matches the RFC requirement below for STREAM_DATA_BLOCKED and DATA_BLOCKED.
            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.1
            //# To keep the
            //# connection from closing, a sender that is flow control limited SHOULD
            //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
            //# has no ack-eliciting packets in flight.
            self.streams_blocked_sync
                .skip_delivery(context.current_time());
            Ok(())
        } else {
            self.streams_blocked_sync.on_transmit(stream_id, context)
        }
    }

    #[inline]
    pub fn close(&mut self) {
        self.wake_all();
        self.streams_blocked_sync.stop_sync();
        self.check_integrity();
    }

    #[inline]
    fn check_integrity(&self) {
        debug_assert!(
            self.closed_streams <= self.opened_streams,
            "Cannot close more streams than previously opened."
        );
        debug_assert!(
            self.open_stream_count() <= self.local_initiated_concurrent_stream_limit,
            "Cannot have more outgoing streams open concurrently than \
                    the local_initiated_concurrent_stream_limit. {:#?}",
            self
        );
    }
}

impl timer::Provider for OutgoingController {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.streams_blocked_sync.timers(query)?;
        Ok(())
    }
}

impl transmission::interest::Provider for OutgoingController {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.streams_blocked_sync.transmission_interest(query)
    }
}

/// Writes the `STREAMS_BLOCKED` frames.
#[derive(Debug, Default)]
pub(super) struct StreamsBlockedToFrameWriter {}

impl ValueToFrameWriter<VarInt> for StreamsBlockedToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&StreamsBlocked {
            stream_type: stream_id.stream_type(),
            stream_limit: value,
        })
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
// Send a MAX_STREAMS frame whenever 1/10th of the window has been closed
pub(super) const MAX_STREAMS_SYNC_FRACTION: VarInt = VarInt::from_u8(10);
//= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
//# Maximum Streams:  A count of the cumulative number of streams of the
//# corresponding type that can be opened over the lifetime of the
//# connection.  This value cannot exceed 2^60, as it is not possible
//# to encode stream IDs larger than 2^62-1.
// Safety: 2^60 is less than MAX_VARINT_VALUE
const MAX_STREAMS_MAX_VALUE: VarInt = unsafe { VarInt::new_unchecked(1 << 60) };

/// The IncomingController controls streams initiated by the peer
#[derive(Debug)]
struct IncomingController {
    peer_initiated_concurrent_stream_limit: VarInt,
    max_streams_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    opened_streams: VarInt,
    closed_streams: VarInt,
}

impl IncomingController {
    fn new(peer_initiated_concurrent_stream_limit: VarInt) -> Self {
        Self {
            peer_initiated_concurrent_stream_limit,
            max_streams_sync: IncrementalValueSync::new(
                peer_initiated_concurrent_stream_limit,
                peer_initiated_concurrent_stream_limit,
                peer_initiated_concurrent_stream_limit / MAX_STREAMS_SYNC_FRACTION,
            ),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
        }
    }

    /// This method is called when the remote peer wishes to open a new stream.
    ///
    /// A `STREAM_LIMIT_ERROR` will be returned if the peer has exceeded the stream limits
    /// that were communicated by transport parameters or MAX_STREAMS frames.
    #[inline]
    fn on_remote_open_stream(&self, stream_id: StreamId) -> Result<(), transport::Error> {
        // open a total of allowed_limit streams
        let allowed_limit = self.max_streams_sync.latest_value().as_u64();
        // open maximum of allowed_streams; streams as 0-indexed
        let allowed_streams = allowed_limit
            .checked_sub(1)
            .ok_or(transport::Error::STREAM_LIMIT_ERROR)?;

        let max_stream_id = StreamId::nth(
            stream_id.initiator(),
            stream_id.stream_type(),
            allowed_streams,
        )
        .expect("max_streams is limited to MAX_STREAMS_MAX_VALUE");

        if stream_id > max_stream_id {
            dbg!(
                "{} {} {:?} {:?}",
                allowed_limit,
                allowed_streams,
                max_stream_id,
                stream_id
            );
            //= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
            //# Endpoints MUST NOT exceed the limit set by their peer.  An endpoint
            //# that receives a frame with a stream ID exceeding the limit it has
            //# sent MUST treat this as a connection error of type
            //# STREAM_LIMIT_ERROR; see Section 11 for details on error handling.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
            //# An endpoint MUST terminate a connection
            //# with an error of type STREAM_LIMIT_ERROR if a peer opens more streams
            //# than was permitted.
            return Err(transport::Error::STREAM_LIMIT_ERROR);
        }
        Ok(())
    }

    #[inline]
    fn on_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        self.on_remote_open_stream(stream_id)?;
        self.opened_streams += 1;

        self.check_integrity();
        Ok(())
    }

    #[inline]
    fn on_close_stream(&mut self) {
        self.closed_streams += 1;

        let max_streams = self
            .closed_streams
            .saturating_add(self.peer_initiated_concurrent_stream_limit)
            .min(MAX_STREAMS_MAX_VALUE);
        self.max_streams_sync.update_latest_value(max_streams);

        self.check_integrity();
    }

    /// Returns the number of streams currently open
    #[inline]
    fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_ack(ack_set);
        self.check_integrity();
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_loss(ack_set);
        self.check_integrity();
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.max_streams_sync.on_transmit(stream_id, context)
    }

    #[inline]
    pub fn close(&mut self) {
        self.max_streams_sync.stop_sync();
        self.check_integrity();
    }

    #[inline]
    fn check_integrity(&self) {
        debug_assert!(
            self.closed_streams <= self.opened_streams,
            "Cannot close more streams than previously opened."
        );
        debug_assert!(
            self.open_stream_count() <= self.peer_initiated_concurrent_stream_limit,
            "Cannot have more incoming streams open concurrently than \
                    the peer_initiated_concurrent_stream_limit. {:#?}",
            self
        );
    }
}

impl transmission::interest::Provider for IncomingController {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.max_streams_sync.transmission_interest(query)
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
    pub fn available_outgoing_stream_capacity(&self, stream_type: StreamType) -> VarInt {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.outgoing.available_stream_capacity(),
            StreamType::Unidirectional => self.uni_controller.outgoing.available_stream_capacity(),
        }
    }

    pub fn max_streams_latest_value(&self, stream_type: StreamType) -> VarInt {
        match stream_type {
            StreamType::Bidirectional => self
                .bidi_controller
                .incoming
                .max_streams_sync
                .latest_value(),
            StreamType::Unidirectional => {
                self.uni_controller.incoming.max_streams_sync.latest_value()
            }
        }
    }
}
