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
    stream,
    stream::{StreamId, StreamType},
    time::{timer, Timestamp},
    transport,
    transport::parameters::InitialFlowControlLimits,
};

pub use remote_initiated::MAX_STREAMS_SYNC_FRACTION;

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
            bidi_controller: ControllerPair {
                stream_id: StreamId::initial(local_endpoint_type, StreamType::Bidirectional),
                local_initiated: LocalInitiated::new(
                    initial_peer_limits.max_streams_bidi,
                    initial_local_limits.max_streams_bidi,
                ),
                remote_initiated: RemoteInitiated::new(initial_local_limits.max_streams_bidi),
            },
            uni_controller: ControllerPair {
                stream_id: StreamId::initial(local_endpoint_type, StreamType::Unidirectional),
                local_initiated: LocalInitiated::new(
                    initial_peer_limits.max_streams_uni,
                    stream_limits.max_open_local_unidirectional_streams,
                ),
                remote_initiated: RemoteInitiated::new(initial_local_limits.max_streams_uni),
            },
        }
    }

    /// This method is called when a `MAX_STREAMS` frame is received,
    /// which signals an increase in the available streams budget.
    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
        match frame.stream_type {
            StreamType::Bidirectional => self.bidi_controller.local_initiated.on_max_streams(frame),
            StreamType::Unidirectional => self.uni_controller.local_initiated.on_max_streams(frame),
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
                .local_initiated
                .poll_open_stream(&mut open_tokens.bidirectional, context),
            StreamType::Unidirectional => self
                .uni_controller
                .local_initiated
                .poll_open_stream(&mut open_tokens.unidirectional, context),
        }
    }

    /// This method is called when the remote peer wishes to open a new stream.
    ///
    /// A `STREAM_LIMIT_ERROR` will be returned if the peer has exceeded the stream limits
    /// that were communicated by transport parameters or MAX_STREAMS frames.
    pub fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        match stream_id.stream_type() {
            StreamType::Bidirectional => self
                .bidi_controller
                .remote_initiated
                .on_remote_open_stream(stream_id),
            StreamType::Unidirectional => self
                .uni_controller
                .remote_initiated
                .on_remote_open_stream(stream_id),
        }
    }

    /// This method is called whenever a stream is opened, regardless of which side initiated.
    pub fn on_open_stream(&mut self, stream_id: StreamId) {
        match self.direction(stream_id) {
            StreamDirection::Bidirectional => self.bidi_controller.on_open_stream(),
            StreamDirection::Outgoing => self.uni_controller.local_initiated.on_open_stream(),
            StreamDirection::Incoming => self.uni_controller.remote_initiated.on_open_stream(),
        }
    }

    /// This method is called whenever a stream is closed.
    pub fn on_close_stream(&mut self, stream_id: StreamId) {
        match self.direction(stream_id) {
            StreamDirection::Bidirectional => self.bidi_controller.on_close_stream(),
            StreamDirection::Outgoing => self.uni_controller.local_initiated.on_close_stream(),
            StreamDirection::Incoming => self.uni_controller.remote_initiated.on_close_stream(),
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
            .local_initiated
            .update_sync_period(blocked_sync_period);
        self.uni_controller
            .local_initiated
            .update_sync_period(blocked_sync_period);
    }

    /// Queries the component for any local_initiated frames that need to get sent
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

/// The controller pair consists of both local_initiated and remote_initiated
/// controllers that are both notified when a stream is opened, regardless
/// of which side initiated the stream.
#[derive(Debug)]
struct ControllerPair {
    stream_id: StreamId,
    local_initiated: LocalInitiated,
    remote_initiated: RemoteInitiated,
}

impl ControllerPair {
    #[inline]
    fn on_open_stream(&mut self) {
        self.local_initiated.on_open_stream();
        self.remote_initiated.on_open_stream();
    }

    #[inline]
    fn on_close_stream(&mut self) {
        self.local_initiated.on_close_stream();
        self.remote_initiated.on_close_stream();
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.remote_initiated.on_packet_ack(ack_set);
        self.local_initiated.on_packet_ack(ack_set);
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.remote_initiated.on_packet_loss(ack_set);
        self.local_initiated.on_packet_loss(ack_set);
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.local_initiated.on_timeout(now);
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Only the stream_type from the StreamId is transmitted
        self.remote_initiated.on_transmit(self.stream_id, context)?;
        self.local_initiated.on_transmit(self.stream_id, context)
    }

    /// This method is called when the stream manager is closed. All wakers will be woken
    /// to unblock waiting tasks.
    pub fn close(&mut self) {
        self.local_initiated.close();
        self.remote_initiated.close();
    }
}

impl timer::Provider for ControllerPair {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.local_initiated.timers(query)?;
        Ok(())
    }
}

impl transmission::interest::Provider for ControllerPair {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.remote_initiated.transmission_interest(query)?;
        self.local_initiated.transmission_interest(query)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::varint::VarInt;

    impl Controller {
        pub fn available_outgoing_stream_capacity(&self, stream_type: StreamType) -> VarInt {
            match stream_type {
                StreamType::Bidirectional => self
                    .bidi_controller
                    .local_initiated
                    .available_stream_capacity(),
                StreamType::Unidirectional => self
                    .uni_controller
                    .local_initiated
                    .available_stream_capacity(),
            }
        }

        pub fn max_streams_latest_value(&self, stream_type: StreamType) -> VarInt {
            match stream_type {
                StreamType::Bidirectional => self.bidi_controller.remote_initiated.latest_limit(),
                StreamType::Unidirectional => self.uni_controller.remote_initiated.latest_limit(),
            }
        }
    }
}
