// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::PacketNumberSpace;
use s2n_codec::EncoderValue;

/// Trait to retrieve the number of bytes that the frame progresses the connection by
/// within the given packet number space
pub trait ConnectionProgress {
    #[inline]
    fn bytes_progressed(&self, _space: PacketNumberSpace) -> usize {
        0
    }
}

impl<AckRanges> ConnectionProgress for crate::frame::Ack<AckRanges> {}
impl ConnectionProgress for crate::frame::ConnectionClose<'_> {}
impl<Data: EncoderValue> ConnectionProgress for crate::frame::Crypto<Data> {
    #[inline]
    fn bytes_progressed(&self, space: PacketNumberSpace) -> usize {
        match space {
            PacketNumberSpace::Initial | PacketNumberSpace::Handshake => self.encoding_size(),
            // Crypto frames in the ApplicationData space do not progress the connection
            PacketNumberSpace::ApplicationData => 0,
        }
    }
}
impl ConnectionProgress for crate::frame::DataBlocked {}
impl ConnectionProgress for crate::frame::HandshakeDone {}
impl ConnectionProgress for crate::frame::MaxData {}
impl ConnectionProgress for crate::frame::MaxStreamData {}
impl ConnectionProgress for crate::frame::MaxStreams {}
impl ConnectionProgress for crate::frame::NewConnectionId<'_> {}
impl ConnectionProgress for crate::frame::NewToken<'_> {}
impl ConnectionProgress for crate::frame::Padding {}
impl ConnectionProgress for crate::frame::PathChallenge<'_> {}
impl ConnectionProgress for crate::frame::PathResponse<'_> {}
impl ConnectionProgress for crate::frame::Ping {}
impl ConnectionProgress for crate::frame::ResetStream {}
impl ConnectionProgress for crate::frame::RetireConnectionId {}
impl ConnectionProgress for crate::frame::StopSending {}
impl ConnectionProgress for crate::frame::StreamsBlocked {}
impl ConnectionProgress for crate::frame::StreamDataBlocked {}
impl<Data: EncoderValue> ConnectionProgress for crate::frame::Stream<Data> {
    #[inline]
    fn bytes_progressed(&self, _space: PacketNumberSpace) -> usize {
        self.encoding_size()
    }
}
