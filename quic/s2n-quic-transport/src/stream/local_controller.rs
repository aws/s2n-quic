// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll, Waker};
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

    pub fn poll_open_stream(&mut self, stream_type: StreamType, context: &Context) -> Poll<()> {
        match stream_type {
            StreamType::Bidirectional => self.outgoing_bidi_controller.poll_open_stream(context),
            StreamType::Unidirectional => self.outgoing_uni_controller.poll_open_stream(context),
        }
    }

    pub fn close(&mut self) {
        self.outgoing_bidi_controller.wake_all();
        self.outgoing_uni_controller.wake_all();
    }
}

const WAKERS_INITIAL_CAPACITY: usize = 5;

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

        let unblocked_wakers_count = self
            .wakers
            .len()
            .min(self.available_streams.as_u64() as usize);

        // Wake the wakers that have been unblocked by this additional stream opening credit
        self.wakers
            .drain(..unblocked_wakers_count)
            .for_each(|waker| waker.wake());
    }

    fn poll_open_stream(&mut self, context: &Context) -> Poll<()> {
        if self.available_streams < VarInt::from_u32(1) {
            // Store a waker that can be woken when we get more credit
            self.wakers.push(context.waker().clone());
            return Poll::Pending;
        }

        self.available_streams -= 1;
        Poll::Ready(())
    }

    fn wake_all(&mut self) {
        self.wakers
            .drain(..self.wakers.len())
            .for_each(|waker| waker.wake())
    }
}
