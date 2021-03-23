// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Waker};
use s2n_quic_core::{
    frame::MaxStreams, stream::StreamType, transport::parameters::InitialFlowControlLimits,
    varint::VarInt,
};
use smallvec::SmallVec;

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

    pub fn poll_open_stream(
        &mut self,
        stream_type: StreamType,
        context: &Context,
    ) -> StreamOpenStatus {
        match stream_type {
            StreamType::Bidirectional => self.outgoing_bidi_controller.poll_open_stream(context),
            StreamType::Unidirectional => self.outgoing_uni_controller.poll_open_stream(context),
        }
    }
}

const WAKERS_INITIAL_CAPACITY: usize = 5;

pub enum StreamOpenStatus {
    Success,
    Blocked,
}

#[derive(Debug)]
struct Controller {
    maximum_streams: VarInt,
    available_streams: VarInt,
    wakers: SmallVec<[Waker; WAKERS_INITIAL_CAPACITY]>,
}

impl Controller {
    fn new(initial_maximum_streams: VarInt) -> Self {
        Self {
            maximum_streams: initial_maximum_streams,
            available_streams: initial_maximum_streams,
            wakers: SmallVec::new(),
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

        // Wake all the wakers now that we have more credit to open more streams
        self.wakers.iter().for_each(|waker| waker.wake_by_ref());
        self.wakers.clear();
    }

    fn poll_open_stream(&mut self, context: &Context) -> StreamOpenStatus {
        if self.available_streams < VarInt::from_u32(1) {
            // Store a waker that can be woken when we get more credit
            self.wakers.push(context.waker().clone());
            return StreamOpenStatus::Blocked;
        }

        self.available_streams -= 1;
        self.wakers.clear();
        StreamOpenStatus::Success
    }
}
