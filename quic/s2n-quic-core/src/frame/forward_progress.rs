// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::EncoderValue;

/// Trait to retrieve the number of bytes that the frame progresses the connection by
pub trait ForwardProgress {
    #[inline]
    fn bytes_progressed(&self) -> usize {
        0
    }
}

impl<AckRanges> ForwardProgress for crate::frame::Ack<AckRanges> {}
impl ForwardProgress for crate::frame::ConnectionClose<'_> {}
impl<Data> ForwardProgress for crate::frame::Crypto<Data> {}
impl ForwardProgress for crate::frame::DataBlocked {}
impl ForwardProgress for crate::frame::HandshakeDone {}
impl ForwardProgress for crate::frame::MaxData {}
impl ForwardProgress for crate::frame::MaxStreamData {}
impl ForwardProgress for crate::frame::MaxStreams {}
impl ForwardProgress for crate::frame::NewConnectionId<'_> {}
impl ForwardProgress for crate::frame::NewToken<'_> {}
impl ForwardProgress for crate::frame::Padding {}
impl ForwardProgress for crate::frame::PathChallenge<'_> {}
impl ForwardProgress for crate::frame::PathResponse<'_> {}
impl ForwardProgress for crate::frame::Ping {}
impl ForwardProgress for crate::frame::ResetStream {}
impl ForwardProgress for crate::frame::RetireConnectionId {}
impl ForwardProgress for crate::frame::StopSending {}
impl ForwardProgress for crate::frame::StreamsBlocked {}
impl ForwardProgress for crate::frame::StreamDataBlocked {}
impl<Data: EncoderValue> ForwardProgress for crate::frame::Stream<Data> {
    #[inline]
    fn bytes_progressed(&self) -> usize {
        self.encoding_size()
    }
}
