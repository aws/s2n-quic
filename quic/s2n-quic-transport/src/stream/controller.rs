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
use s2n_quic_core::{
    ack, endpoint,
    frame::MaxStreams,
    stream::{
        self,
        iter::StreamIter,
        limits::{LocalBidirectional, LocalUnidirectional},
        StreamId, StreamType,
    },
    time::{timer, Timestamp},
    transport,
    transport::parameters::InitialFlowControlLimits,
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
    local_bidi_controller: LocalInitiated<LocalBidirectional>,
    remote_bidi_controller: RemoteInitiated,
    local_uni_controller: LocalInitiated<LocalUnidirectional>,
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
    ) -> Self {
        Self {
            local_endpoint_type,
            local_bidi_controller: LocalInitiated::new(
                initial_peer_limits.max_streams_bidi,
                stream_limits.max_open_local_bidirectional_streams,
            ),
            remote_bidi_controller: RemoteInitiated::new(initial_local_limits.max_streams_bidi),
            local_uni_controller: LocalInitiated::new(
                initial_peer_limits.max_streams_uni,
                stream_limits.max_open_local_unidirectional_streams,
            ),
            remote_uni_controller: RemoteInitiated::new(initial_local_limits.max_streams_uni),
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

    /// This method is called when the local application wishes to open a new stream.
    ///
    /// This API requires that only one stream is opened per invocation and must be
    /// called the next stream id of a type.
    ///
    /// `Poll::Pending` is returned when there isn't available capacity to open a stream,
    /// either because of local initiated concurrency limits or the peer's stream limits.
    /// If `Poll::Pending` is returned, the waker in the given `context` will be woken
    /// when additional stream capacity becomes available.
    pub fn poll_open_local_stream(
        &mut self,
        stream_id: StreamId,
        open_tokens: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<StreamId> {
        if cfg!(debug_assertions) {
            match self.direction(stream_id) {
                StreamDirection::RemoteInitiatedBidirectional
                | StreamDirection::RemoteInitiatedUnidirectional => {
                    panic!("should only be called for locally initiated streams")
                }
                _ => (),
            }
        }

        let poll = match stream_id.stream_type() {
            StreamType::Bidirectional => self
                .local_bidi_controller
                .poll_open_stream(&mut open_tokens.bidirectional, context),
            StreamType::Unidirectional => self
                .local_uni_controller
                .poll_open_stream(&mut open_tokens.unidirectional, context),
        };

        match poll {
            Poll::Ready(_) => {
                // only open streams if there is sufficient capacity based on limits
                self.on_open_stream(stream_id);
                Poll::Ready(stream_id)
            }
            Poll::Pending => Poll::Pending,
        }
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
        if cfg!(debug_assertions) {
            match self.direction(stream_iter.max_stream_id()) {
                StreamDirection::LocalInitiatedBidirectional
                | StreamDirection::LocalInitiatedUnidirectional => {
                    panic!("should only be called for remote initiated streams")
                }
                _ => (),
            }
        }

        // return early if there is not sufficient capacity based on limits
        match stream_iter.max_stream_id().stream_type() {
            StreamType::Bidirectional => self
                .remote_bidi_controller
                .on_remote_open_stream(stream_iter.max_stream_id())?,
            StreamType::Unidirectional => self
                .remote_uni_controller
                .on_remote_open_stream(stream_iter.max_stream_id())?,
        }

        for stream_id in stream_iter {
            self.on_open_stream(stream_id);
        }
        Ok(())
    }

    /// This method is called whenever a stream is opened, regardless of
    /// which side initiated.
    ///
    /// The caller is responsible for performing stream capacity checks
    /// prior to calling this function.
    fn on_open_stream(&mut self, stream_id: StreamId) {
        match self.direction(stream_id) {
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

    /// Queries the component for any local_initiated frames that need to get sent
    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        if !self.has_transmission_interest() {
            return Ok(());
        }

        // Only the stream_type from the StreamId is transmitted
        let stream_id = StreamId::initial(self.local_endpoint_type, StreamType::Bidirectional);
        self.local_bidi_controller.on_transmit(stream_id, context)?;
        self.remote_bidi_controller
            .on_transmit(stream_id, context)?;

        // Only the stream_type from the StreamId is transmitted
        let stream_id = StreamId::initial(self.local_endpoint_type, StreamType::Unidirectional);
        self.remote_uni_controller.on_transmit(stream_id, context)?;
        self.local_uni_controller.on_transmit(stream_id, context)?;

        Ok(())
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.local_bidi_controller.on_timeout(now);
        self.local_uni_controller.on_timeout(now);
    }

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
        self.local_uni_controller.timers(query)?;
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
    LocalInitiatedBidirectional,
    RemoteInitiatedBidirectional,
    LocalInitiatedUnidirectional,
    RemoteInitiatedUnidirectional,
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

        pub fn available_remote_intiated_stream_capacity(&self, stream_type: StreamType) -> VarInt {
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
