// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    connection, frame::MaxStreams, stream::StreamType,
    transport::parameters::InitialFlowControlLimits, varint::VarInt,
};

#[derive(Debug)]
pub struct LocalController {
    outgoing_bidi_controller: Controller,
    outgoing_uni_controller: Controller,
}

impl LocalController {
    pub fn new(initial_peer_limits: InitialFlowControlLimits) -> Self {
        Self {
            outgoing_bidi_controller: Controller::new(initial_peer_limits.max_streams_bidi),
            outgoing_uni_controller: Controller::new(initial_peer_limits.max_streams_uni),
        }
    }

    pub fn on_max_streams(&mut self, frame: &MaxStreams) {
        match frame.stream_type {
            StreamType::Bidirectional => self.outgoing_bidi_controller.on_max_streams(frame),
            StreamType::Unidirectional => self.outgoing_uni_controller.on_max_streams(frame),
        }
    }

    pub fn try_open_stream(&mut self, stream_type: StreamType) -> Result<(), connection::Error> {
        match stream_type {
            StreamType::Bidirectional => self.outgoing_bidi_controller.try_open_stream(),
            StreamType::Unidirectional => self.outgoing_uni_controller.try_open_stream(),
        }
    }
}

#[derive(Debug)]
struct Controller {
    maximum_streams: VarInt,
    available_streams: VarInt,
}

impl Controller {
    fn new(initial_maximum_streams: VarInt) -> Self {
        Self {
            maximum_streams: initial_maximum_streams,
            available_streams: initial_maximum_streams,
        }
    }

    fn on_max_streams(&mut self, frame: &MaxStreams) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.6
        //# A receiver MUST
        //# ignore any MAX_STREAMS frame that does not increase the stream limit.
        if self.maximum_streams >= frame.maximum_streams {
            return;
        }

        let increment = frame.maximum_streams - self.maximum_streams;
        self.maximum_streams = frame.maximum_streams;
        self.available_streams += increment;
    }

    fn try_open_stream(&mut self) -> Result<(), connection::Error> {
        if self.available_streams < VarInt::from_u32(1) {
            return Err(connection::Error::StreamBlocked);
        }

        self.available_streams -= 1;
        Ok(())
    }
}
