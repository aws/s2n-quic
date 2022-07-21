// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::OnTransmitError,
    sync::{IncrementalValueSync, ValueToFrameWriter},
    transmission,
    transmission::WriteContext,
};
use s2n_quic_core::{
    ack, frame::MaxStreams, packet::number::PacketNumber, stream::StreamId, transport,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-4.6
//# An endpoint MUST NOT wait
//# to receive this signal before advertising additional credit, since
//# doing so will mean that the peer will be blocked for at least an
//# entire round trip
// Send a MAX_STREAMS frame whenever 1/10th of the window has been closed
pub const MAX_STREAMS_SYNC_FRACTION: VarInt = VarInt::from_u8(10);
//= https://www.rfc-editor.org/rfc/rfc9000#section-19.11
//# Maximum Streams:  A count of the cumulative number of streams of the
//# corresponding type that can be opened over the lifetime of the
//# connection.  This value cannot exceed 2^60, as it is not possible
//# to encode stream IDs larger than 2^62-1.
// Safety: 2^60 is less than MAX_VARINT_VALUE
const MAX_STREAMS_MAX_VALUE: VarInt = unsafe { VarInt::new_unchecked(1 << 60) };

/// The RemoteInitiated controller controls streams initiated by the peer
#[derive(Debug)]
pub(super) struct RemoteInitiated {
    /// The max stream limit specified by the local endpoint.
    ///
    /// Used to calculate updated max_streams_sync value as the peer
    /// closes streams.
    max_local_limit: VarInt,
    /// Responsible for advertising updated max stream frames as the
    /// peer closes streams
    max_streams_sync: IncrementalValueSync<VarInt, MaxStreamsToFrameWriter>,
    opened_streams: VarInt,
    closed_streams: VarInt,
}

impl RemoteInitiated {
    pub fn new(max_local_limit: VarInt) -> Self {
        Self {
            max_local_limit,
            max_streams_sync: IncrementalValueSync::new(
                max_local_limit,
                max_local_limit,
                max_local_limit / MAX_STREAMS_SYNC_FRACTION,
            ),
            opened_streams: VarInt::from_u8(0),
            closed_streams: VarInt::from_u8(0),
        }
    }

    pub fn on_remote_open_stream(&mut self, stream_id: StreamId) -> Result<(), transport::Error> {
        // get the total number of streams that are allowed
        let max_allowed_stream_limit = self.max_streams_sync.latest_value().as_u64();

        // since streams are 0-indexed, using `max_allowed_stream_limit` to calculate
        // the stream_id gives 1 stream_id greater than the allowed limit
        let not_allowed_stream_id = StreamId::nth(
            stream_id.initiator(),
            stream_id.stream_type(),
            max_allowed_stream_limit,
        )
        .expect("max_streams is limited to MAX_STREAMS_MAX_VALUE");

        if stream_id >= not_allowed_stream_id {
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

    pub fn on_open_stream(&mut self) {
        self.opened_streams += 1;

        self.check_integrity();
    }

    pub fn on_close_stream(&mut self) {
        self.closed_streams += 1;

        let max_streams = self
            .closed_streams
            .saturating_add(self.max_local_limit)
            .min(MAX_STREAMS_MAX_VALUE);
        self.max_streams_sync.update_latest_value(max_streams);

        self.check_integrity();
    }

    /// Returns the number of streams currently open
    pub fn open_stream_count(&self) -> VarInt {
        self.opened_streams - self.closed_streams
    }

    #[inline]
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_ack(ack_set)
    }

    #[inline]
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.max_streams_sync.on_packet_loss(ack_set)
    }

    #[inline]
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        self.max_streams_sync.on_transmit(stream_id, context)
    }

    pub fn close(&mut self) {
        self.max_streams_sync.stop_sync();
    }

    #[inline]
    fn check_integrity(&self) {
        if cfg!(debug_assertions) {
            assert!(
                self.closed_streams <= self.opened_streams,
                "Cannot close more streams than previously opened"
            );
            assert!(
                self.open_stream_count() <= self.max_local_limit,
                "Cannot have more incoming streams open concurrently than
                the max_local_limit"
            );
        }
    }

    #[cfg(test)]
    pub fn latest_limit(&self) -> VarInt {
        self.max_streams_sync.latest_value()
    }
}

impl transmission::interest::Provider for RemoteInitiated {
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
