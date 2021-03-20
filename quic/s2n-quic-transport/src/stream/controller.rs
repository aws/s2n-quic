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
    transport::error::TransportError,
    varint::VarInt,
};

// Send a MAX_STREAMS frame whenever 10% of the window has been closed
const MAX_STREAMS_SYNC_PERCENTAGE: VarInt = VarInt::from_u8(10);

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

struct IncomingController {
    pub(super) read_window_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    stream_type: StreamType,
    window_size: VarInt,
    closed_streams: VarInt,
    opened_streams: VarInt,
}

impl IncomingController {
    pub fn new(window_size: VarInt, stream_type: StreamType) -> Self {
        Self {
            read_window_sync: IncrementalValueSync::new(
                window_size,
                window_size,
                window_size / MAX_STREAMS_SYNC_PERCENTAGE,
            ),
            stream_type,
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

        self.read_window_sync
            .update_latest_value(self.closed_streams.saturating_add(self.window_size));
    }

    fn available_streams(&self) -> VarInt {
        self.read_window_sync.latest_value() - self.opened_streams
    }

    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.read_window_sync.on_packet_ack(ack_set)
    }

    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.read_window_sync.on_packet_loss(ack_set)
    }

    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Only the stream_type from the StreamId is transmitted
        self.read_window_sync.on_transmit(
            StreamId::initial(context.local_endpoint_type(), self.stream_type),
            context,
        )
    }
}

impl transmission::interest::Provider for IncomingController {
    fn transmission_interest(&self) -> transmission::Interest {
        self.read_window_sync.transmission_interest()
    }
}
