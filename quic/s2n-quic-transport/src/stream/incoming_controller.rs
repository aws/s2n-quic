// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::OnTransmitError,
    sync::{IncrementalValueSync, ValueToFrameWriter},
    transmission,
    transmission::WriteContext,
};
use s2n_quic_core::{
    ack,
    frame::MaxStreams,
    packet::number::PacketNumber,
    stream::{StreamId, StreamType},
    transport::{error::TransportError, parameters::InitialFlowControlLimits},
    varint::VarInt,
};

// Send a MAX_STREAMS frame whenever 10% of the window has been closed
const MAX_STREAMS_SYNC_PERCENTAGE: VarInt = VarInt::from_u8(10);

struct IncomingController {
    bidi_controller: StreamTypeIncomingController,
    uni_controller: StreamTypeIncomingController,
}

impl IncomingController {
    pub fn new(initial_local_limits: InitialFlowControlLimits) -> Self {
        Self {
            bidi_controller: StreamTypeIncomingController::new(
                initial_local_limits.max_streams_bidi,
            ),
            uni_controller: StreamTypeIncomingController::new(initial_local_limits.max_streams_uni),
        }
    }

    pub fn on_open_stream(&mut self, stream_type: StreamType) -> Result<(), TransportError> {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.on_open_stream(),
            StreamType::Unidirectional => self.uni_controller.on_open_stream(),
        }
    }

    pub fn on_close_stream(&mut self, stream_type: StreamType) {
        match stream_type {
            StreamType::Bidirectional => self.bidi_controller.on_close_stream(),
            StreamType::Unidirectional => self.uni_controller.on_close_stream(),
        }
    }

    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller.window_sync.on_packet_ack(ack_set);
        self.uni_controller.window_sync.on_packet_ack(ack_set);
    }

    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.bidi_controller.window_sync.on_packet_loss(ack_set);
        self.uni_controller.window_sync.on_packet_loss(ack_set);
    }

    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Only the stream_type from the StreamId is transmitted
        self.bidi_controller.window_sync.on_transmit(
            StreamId::initial(context.local_endpoint_type(), StreamType::Bidirectional),
            context,
        )?;
        self.uni_controller.window_sync.on_transmit(
            StreamId::initial(context.local_endpoint_type(), StreamType::Unidirectional),
            context,
        )
    }
}

impl transmission::interest::Provider for IncomingController {
    fn transmission_interest(&self) -> transmission::Interest {
        self.bidi_controller.window_sync.transmission_interest()
            + self.uni_controller.window_sync.transmission_interest()
    }
}

/// Writes the `MAX_STREAMS` frames based on the stream control window.
#[derive(Default)]
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

struct StreamTypeIncomingController {
    window_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    window_size: VarInt,
    closed_streams: VarInt,
    opened_streams: VarInt,
}

impl StreamTypeIncomingController {
    fn new(window_size: VarInt) -> Self {
        Self {
            window_sync: IncrementalValueSync::new(
                window_size,
                window_size,
                window_size / MAX_STREAMS_SYNC_PERCENTAGE,
            ),
            window_size,
            closed_streams: VarInt::from_u32(0),
            opened_streams: VarInt::from_u32(0),
        }
    }

    fn on_open_stream(&mut self) -> Result<(), TransportError> {
        if self.available_streams() < VarInt::from_u32(1) {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
            //# An endpoint
            //# that receives a frame with a stream ID exceeding the limit it has
            //# sent MUST treat this as a connection error of type STREAM_LIMIT_ERROR
            //# (Section 11).
            return Err(TransportError::STREAM_LIMIT_ERROR);
        }
        self.opened_streams += 1;
        Ok(())
    }

    fn on_close_stream(&mut self) {
        self.closed_streams += 1;
        debug_assert!(
            self.closed_streams <= self.opened_streams,
            "Can not close more streams than previously opened"
        );

        self.window_sync
            .update_latest_value(self.closed_streams.saturating_add(self.window_size));
    }

    fn available_streams(&self) -> VarInt {
        self.window_sync.latest_value() - self.opened_streams
    }
}
