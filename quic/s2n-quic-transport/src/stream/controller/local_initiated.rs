// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::open_token,
    contexts::OnTransmitError,
    sync::{OnceSync, PeriodicSync, ValueToFrameWriter},
    transmission,
    transmission::WriteContext,
};
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use s2n_quic_core::{
    ack,
    frame::{self, MaxStreams, StreamsBlocked},
    packet::number::PacketNumber,
    stream::{limits::LocalLimits, StreamId},
    time::{timer, Timestamp},
    varint::VarInt,
};
use smallvec::SmallVec;

// The amount of wakers that may be tracked before allocating to the heap.
const WAKERS_INITIAL_CAPACITY: usize = 5;

/// The LocalInitiated controller controls streams initiated locally
#[derive(Debug)]
pub(super) struct LocalInitiated<L: LocalLimits, OpenNotify: OpenNotifyBehavior> {
    /// The max stream limit specified by the local endpoint.
    ///
    /// Used to restrict the number of concurrent streams the local
    /// connection can open.
    max_local_limit: L,
    /// The cumulative stream limit specified by the remote endpoint.
    ///
    /// Can be updated when MAX_STREAMS frame is received.
    peer_cumulative_stream_limit: VarInt,
    wakers: SmallVec<[Waker; WAKERS_INITIAL_CAPACITY]>,
    streams_blocked_sync: PeriodicSync<VarInt, StreamsBlockedToFrameWriter>,
    /// opened_streams is needed to track the latest opened stream since
    /// peer_stream_limit is a cumulative limit.
    opened_streams: VarInt,
    closed_streams: VarInt,
    /// Keeps track of all of the issued open tokens
    token_counter: open_token::Counter,
    /// Keeps track of all of the expired open tokens
    expired_token: open_token::Token,
    open_notify: OpenNotify,
}

impl<L: LocalLimits, OpenNotify: OpenNotifyBehavior> LocalInitiated<L, OpenNotify> {
    pub fn new(initial_peer_maximum_streams: VarInt, max_local_limit: L) -> Self {
        Self {
            max_local_limit,
            peer_cumulative_stream_limit: initial_peer_maximum_streams,
            wakers: SmallVec::new(),
            streams_blocked_sync: PeriodicSync::new(),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
            token_counter: open_token::Counter::new(),
            expired_token: open_token::Token::new(),
            open_notify: Default::default(),
        }
    }

    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
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
    }

    pub fn update_sync_period(&mut self, blocked_sync_period: Duration) {
        self.streams_blocked_sync
            .update_sync_period(blocked_sync_period);
    }

    #[inline]
    pub fn poll_open_stream(
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

            return Poll::Pending;
        }

        // reset the open token since they're no longer blocked
        open_token.clear();

        Poll::Ready(())
    }

    #[inline]
    pub fn on_open_stream(&mut self) {
        self.opened_streams += 1;
        self.open_notify.on_open_stream();

        self.check_integrity();
    }

    pub fn on_close_stream(&mut self) {
        self.closed_streams += 1;

        self.wake_unblocked();
        self.check_integrity();
    }

    /// The number of streams that may be opened by the local application, respecting both
    /// the local concurrent streams limit and the peer's stream limits.
    #[inline]
    pub fn available_stream_capacity(&self) -> VarInt {
        let local_capacity = self
            .max_local_limit
            .as_varint()
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
    #[inline]
    pub fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    pub fn total_open_stream_count(&self) -> VarInt {
        self.opened_streams
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.streams_blocked_sync.on_timeout(now);
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.open_notify.on_packet_ack(ack_set);
        self.streams_blocked_sync.on_packet_ack(ack_set)
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.open_notify.on_packet_loss(ack_set);
        self.streams_blocked_sync.on_packet_loss(ack_set)
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(
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
        } else {
            self.streams_blocked_sync.on_transmit(stream_id, context)?;
        }

        self.open_notify.on_transmit(stream_id, context)?;

        Ok(())
    }

    pub fn close(&mut self) {
        self.wake_all();
        self.streams_blocked_sync.stop_sync();
        self.open_notify.close();
    }

    #[inline]
    pub fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            assert!(
                self.closed_streams <= self.opened_streams,
                "Cannot close more streams than previously opened"
            );
            assert!(
                self.open_stream_count() <= self.max_local_limit.as_varint(),
                "Cannot have more outgoing streams open concurrently than
                the max_local_limit"
            );
        }
    }
}

impl<L: LocalLimits, OpenNotify: OpenNotifyBehavior> timer::Provider
    for LocalInitiated<L, OpenNotify>
{
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.streams_blocked_sync.timers(query)?;
        Ok(())
    }
}

impl<L: LocalLimits, OpenNotify: OpenNotifyBehavior> transmission::interest::Provider
    for LocalInitiated<L, OpenNotify>
{
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.streams_blocked_sync.transmission_interest(query)?;
        self.open_notify.transmission_interest(query)?;
        Ok(())
    }
}

/// Writes the `STREAMS_BLOCKED` frames.
#[derive(Debug, Default)]
pub(super) struct StreamsBlockedToFrameWriter {}

impl ValueToFrameWriter<VarInt> for StreamsBlockedToFrameWriter {
    #[inline]
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

/// Interface for determining what to do for notifying the peer about opening
/// the stream.
///
/// By default, the trait provides no-op implementations for all of the methods.
pub trait OpenNotifyBehavior: Default + transmission::interest::Provider {
    #[inline]
    fn on_open_stream(&mut self) {}

    #[inline]
    fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        let _ = stream_id;
        let _ = context;
        Ok(())
    }

    #[inline]
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        let _ = ack_set;
    }

    #[inline]
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        let _ = ack_set;
    }

    #[inline]
    fn close(&mut self) {}
}

/// Defines the open notify behavior for unidirectional streams
///
/// Since locally-initiated, unidirectional streams can only send data, all of
/// these operations are no-op, as the peer will become aware of the stream once
/// the local application starts sending on it.
#[derive(Debug, Default)]
pub(super) struct OpenNotifyUnidirectional;

impl OpenNotifyBehavior for OpenNotifyUnidirectional {}

impl transmission::interest::Provider for OpenNotifyUnidirectional {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        _query: &mut Q,
    ) -> transmission::interest::Result {
        Ok(())
    }
}

/// Defines the open notify behavior for bidirectional streams
///
/// This will send an empty STREAM frame to the peer for the largest
/// opened, locally-initiated bidirectional stream. This prevents a deadlock
/// where the local application opens the stream and tries to receive on it.
#[derive(Debug, Default)]
pub(super) struct OpenNotifyBidirectional {
    max_opened: OnceSync<(), OpenNotifyFrameWriter>,
}

impl OpenNotifyBehavior for OpenNotifyBidirectional {
    #[inline]
    fn on_open_stream(&mut self) {
        self.max_opened.force_delivery(())
    }

    #[inline]
    fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.max_opened.on_transmit(stream_id, context)
    }

    #[inline]
    fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        let _ = self.max_opened.on_packet_ack(ack_set);
    }

    #[inline]
    fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_opened.on_packet_loss(ack_set)
    }

    #[inline]
    fn close(&mut self) {
        self.max_opened.stop_sync();
    }
}

impl transmission::interest::Provider for OpenNotifyBidirectional {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.max_opened.transmission_interest(query)
    }
}

/// Writes an empty STREAM frame to the peer, indicating the stream has been created
#[derive(Debug, Default)]
struct OpenNotifyFrameWriter;

impl ValueToFrameWriter<()> for OpenNotifyFrameWriter {
    #[inline]
    fn write_value_as_frame<W: WriteContext>(
        &self,
        _value: (),
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&frame::Stream {
            stream_id: stream_id.into(),
            is_last_frame: false,
            is_fin: false,
            offset: VarInt::from_u32(0),
            data: &[0u8; 0][..],
        })
    }
}
