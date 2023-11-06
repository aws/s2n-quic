// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod local_initiated;
mod remote_initiated;

use self::{local_initiated::LocalInitiated, remote_initiated::RemoteInitiated};
use crate::{
    connection,
    contexts::OnTransmitError,
    transmission,
    transmission::{interest::Provider, WriteContext},
};
use core::{
    task::{Context, Poll},
    time::Duration,
};
use futures_core::ready;
use s2n_quic_core::{
    ack, endpoint,
    frame::MaxStreams,
    stream::{self, iter::StreamIter, StreamId, StreamType},
    time::{timer, Timestamp},
    transport,
    transport::parameters::InitialFlowControlLimits,
    varint::VarInt,
};

pub use remote_initiated::MAX_STREAMS_SYNC_FRACTION;

/// This component manages stream concurrency limits.
///
/// It enforces both the local initiated stream limits and the peer initiated
/// stream limits.
///
/// It will also signal an increased max streams once streams have been consumed.
#[derive(Debug)]
pub struct Controller {
    local_endpoint_type: endpoint::Type,
    local_bidi_controller: LocalInitiated<
        stream::limits::LocalBidirectional,
        local_initiated::OpenNotifyBidirectional,
    >,
    remote_bidi_controller: RemoteInitiated,
    local_uni_controller: LocalInitiated<
        stream::limits::LocalUnidirectional,
        local_initiated::OpenNotifyUnidirectional,
    >,
    remote_uni_controller: RemoteInitiated,
}

impl Controller {
    /// Creates a new `stream::Controller`
    ///
    /// The peer will be allowed to open streams up to the given `initial_local_limits`.
    ///
    /// For local_initiated unidirectional streams, the local application will be allowed to open
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
        min_rtt: Duration,
    ) -> Self {
        Self {
            local_endpoint_type,
            local_bidi_controller: LocalInitiated::new(
                initial_peer_limits.max_open_remote_bidirectional_streams,
                stream_limits.max_open_local_bidirectional_streams,
            ),
            remote_bidi_controller: RemoteInitiated::new(
                initial_local_limits.max_open_remote_bidirectional_streams,
                min_rtt,
            ),
            local_uni_controller: LocalInitiated::new(
                initial_peer_limits.max_open_remote_unidirectional_streams,
                stream_limits.max_open_local_unidirectional_streams,
            ),
            remote_uni_controller: RemoteInitiated::new(
                initial_local_limits.max_open_remote_unidirectional_streams,
                min_rtt,
            ),
        }
    }

    /// This method is called when a `MAX_STREAMS` frame is received,
    /// which signals an increase in the available streams budget.
    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
        match frame.stream_type {
            StreamType::Bidirectional => self.local_bidi_controller.on_max_streams(frame),
            StreamType::Unidirectional => self.local_uni_controller.on_max_streams(frame),
        }
    }

    /// This method is called when the local application wishes to open the next stream
    /// of a type (Bidirectional/Unidirectional).
    ///
    /// `Poll::Pending` is returned when there isn't available capacity to open a stream,
    /// either because of local initiated concurrency limits or the peer's stream limits.
    /// If `Poll::Pending` is returned, the waker in the given `context` will be woken
    /// when additional stream capacity becomes available.
    pub fn poll_open_local_stream(
        &mut self,
        stream_type: StreamType,
        open_tokens: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<()> {
        let poll_open = match stream_type {
            StreamType::Bidirectional => self
                .local_bidi_controller
                .poll_open_stream(&mut open_tokens.bidirectional, context),
            StreamType::Unidirectional => self
                .local_uni_controller
                .poll_open_stream(&mut open_tokens.unidirectional, context),
        };

        // returns Pending if there is no capacity available
        ready!(poll_open);

        // only open streams if there is sufficient capacity based on limits
        let direction = self.direction(StreamId::initial(self.local_endpoint_type, stream_type));
        self.on_open_stream(direction);
        Poll::Ready(())
    }

    /// This method is called when the remote peer wishes to open a new stream.
    ///
    /// Opening a Stream also opens all lower Streams of the same type. Therefore
    /// this function validates if there is enough capacity to open all streams.
    ///
    /// A `STREAM_LIMIT_ERROR` will be returned if the peer has exceeded the
    /// stream limits that were communicated by transport parameters or
    /// MAX_STREAMS frames.
    pub fn on_open_remote_stream(
        &mut self,
        stream_iter: StreamIter,
    ) -> Result<(), transport::Error> {
        debug_assert!(
            self.direction(stream_iter.max_stream_id()).is_remote(),
            "should only be called for remote initiated streams"
        );

        // return early if there is not sufficient capacity based on limits
        match stream_iter.max_stream_id().stream_type() {
            StreamType::Bidirectional => self
                .remote_bidi_controller
                .on_remote_open_stream(stream_iter.max_stream_id())?,
            StreamType::Unidirectional => self
                .remote_uni_controller
                .on_remote_open_stream(stream_iter.max_stream_id())?,
        }

        let direction = self.direction(stream_iter.max_stream_id());
        // checked above that there is enough capacity to open all streams
        for _stream_id in stream_iter {
            self.on_open_stream(direction);
        }
        Ok(())
    }

    /// This method is called whenever a stream is opened, regardless of
    /// which side initiated.
    ///
    /// The caller is responsible for performing stream capacity checks
    /// prior to calling this function.
    fn on_open_stream(&mut self, direction: StreamDirection) {
        match direction {
            StreamDirection::LocalInitiatedBidirectional => {
                self.local_bidi_controller.on_open_stream()
            }
            StreamDirection::RemoteInitiatedBidirectional => {
                self.remote_bidi_controller.on_open_stream()
            }
            StreamDirection::LocalInitiatedUnidirectional => {
                self.local_uni_controller.on_open_stream()
            }
            StreamDirection::RemoteInitiatedUnidirectional => {
                self.remote_uni_controller.on_open_stream()
            }
        }
    }

    /// This method is called whenever a stream is closed.
    pub fn on_close_stream(&mut self, stream_id: StreamId) {
        match self.direction(stream_id) {
            StreamDirection::LocalInitiatedBidirectional => {
                self.local_bidi_controller.on_close_stream()
            }
            StreamDirection::RemoteInitiatedBidirectional => {
                self.remote_bidi_controller.on_close_stream()
            }
            StreamDirection::LocalInitiatedUnidirectional => {
                self.local_uni_controller.on_close_stream()
            }
            StreamDirection::RemoteInitiatedUnidirectional => {
                self.remote_uni_controller.on_close_stream()
            }
        }
    }

    /// This method is called when the stream manager is closed. All wakers will be woken
    /// to unblock waiting tasks.
    pub fn close(&mut self) {
        self.local_bidi_controller.close();
        self.remote_bidi_controller.close();
        self.local_uni_controller.close();
        self.remote_uni_controller.close();
    }

    /// This method is called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.local_bidi_controller.on_packet_ack(ack_set);
        self.remote_bidi_controller.on_packet_ack(ack_set);
        self.local_uni_controller.on_packet_ack(ack_set);
        self.remote_uni_controller.on_packet_ack(ack_set);
    }

    /// This method is called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.local_bidi_controller.on_packet_loss(ack_set);
        self.remote_bidi_controller.on_packet_loss(ack_set);
        self.local_uni_controller.on_packet_loss(ack_set);
        self.remote_uni_controller.on_packet_loss(ack_set);
    }

    /// Updates the period at which `STREAMS_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    pub fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.local_bidi_controller
            .update_sync_period(blocked_sync_period);
        self.local_uni_controller
            .update_sync_period(blocked_sync_period);
    }

    pub fn update_min_rtt(&mut self, min_rtt: Duration, now: Timestamp) {
        self.remote_uni_controller.update_min_rtt(min_rtt, now);
        self.remote_bidi_controller.update_min_rtt(min_rtt, now);
    }

    /// Queries the component for any local_initiated frames that need to get sent
    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        if !self.has_transmission_interest() {
            return Ok(());
        }

        let peer_endpoint_type = self.local_endpoint_type.peer_type();

        macro_rules! on_transmit {
            ($controller:ident, $endpoint:expr, $ty:expr) => {
                if let Some(nth) = self
                    .$controller
                    .total_open_stream_count()
                    .checked_sub(VarInt::from_u32(1))
                {
                    if let Some(stream_id) = StreamId::nth($endpoint, $ty, nth.as_u64()) {
                        self.$controller.on_transmit(stream_id, context)?;
                    }
                }
            };
        }

        on_transmit!(
            local_bidi_controller,
            self.local_endpoint_type,
            StreamType::Bidirectional
        );
        on_transmit!(
            remote_bidi_controller,
            peer_endpoint_type,
            StreamType::Bidirectional
        );

        on_transmit!(
            local_uni_controller,
            self.local_endpoint_type,
            StreamType::Unidirectional
        );
        on_transmit!(
            remote_uni_controller,
            peer_endpoint_type,
            StreamType::Unidirectional
        );

        Ok(())
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.local_bidi_controller.on_timeout(now);
        self.remote_bidi_controller.on_timeout(now);
        self.local_uni_controller.on_timeout(now);
        self.remote_uni_controller.on_timeout(now);
    }

    #[inline]
    fn direction(&self, stream_id: StreamId) -> StreamDirection {
        let is_local_initiated = self.local_endpoint_type == stream_id.initiator();
        match (is_local_initiated, stream_id.stream_type()) {
            (true, StreamType::Bidirectional) => StreamDirection::LocalInitiatedBidirectional,
            (true, StreamType::Unidirectional) => StreamDirection::LocalInitiatedUnidirectional,
            (false, StreamType::Bidirectional) => StreamDirection::RemoteInitiatedBidirectional,
            (false, StreamType::Unidirectional) => StreamDirection::RemoteInitiatedUnidirectional,
        }
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.local_bidi_controller.timers(query)?;
        self.remote_bidi_controller.timers(query)?;
        self.local_uni_controller.timers(query)?;
        self.remote_uni_controller.timers(query)?;
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
        self.local_bidi_controller.transmission_interest(query)?;
        self.remote_bidi_controller.transmission_interest(query)?;
        self.local_uni_controller.transmission_interest(query)?;
        self.remote_uni_controller.transmission_interest(query)?;
        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
enum StreamDirection {
    // A bidirectional stream opened by the local application to send
    // and receive data
    LocalInitiatedBidirectional,

    // A bidirectional stream opened by the peer to send and receive
    // data
    RemoteInitiatedBidirectional,

    // A unidirectional stream opened by the local application to send
    // data
    LocalInitiatedUnidirectional,

    // A unidirectional stream opened by the peer to send data
    RemoteInitiatedUnidirectional,
}

impl StreamDirection {
    fn is_remote(&self) -> bool {
        match self {
            StreamDirection::LocalInitiatedBidirectional => false,
            StreamDirection::RemoteInitiatedBidirectional => true,
            StreamDirection::LocalInitiatedUnidirectional => false,
            StreamDirection::RemoteInitiatedUnidirectional => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::varint::VarInt;

    impl Controller {
        pub fn available_local_initiated_stream_capacity(&self, stream_type: StreamType) -> VarInt {
            match stream_type {
                StreamType::Bidirectional => self.local_bidi_controller.available_stream_capacity(),
                StreamType::Unidirectional => self.local_uni_controller.available_stream_capacity(),
            }
        }

        pub fn remote_initiated_max_streams_latest_value(&self, stream_type: StreamType) -> VarInt {
            match stream_type {
                StreamType::Bidirectional => self.remote_bidi_controller.latest_limit(),
                StreamType::Unidirectional => self.remote_uni_controller.latest_limit(),
            }
        }

        pub fn available_remote_initiated_stream_capacity(
            &self,
            stream_type: StreamType,
        ) -> VarInt {
            match stream_type {
                StreamType::Bidirectional => {
                    self.remote_initiated_max_streams_latest_value(stream_type)
                        - self.remote_bidi_controller.open_stream_count()
                }
                StreamType::Unidirectional => {
                    self.remote_initiated_max_streams_latest_value(stream_type)
                        - self.remote_uni_controller.open_stream_count()
                }
            }
        }
    }
}

#[cfg(test)]
mod fuzz_target;
