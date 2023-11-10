// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint,
    event::{self, IntoEvent},
    frame::{ack::AckRanges as AckRangesTrait, ack_elicitation::AckElicitation, Ack, FrameTrait},
    packet::number::PacketNumber,
    time::Timestamp,
    transmission,
};
use s2n_codec::encoder::EncoderValue;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Context information that is passed to `on_transmit` calls on Streams
pub trait Writer {
    /// Returns the current point of time
    fn current_time(&self) -> Timestamp;

    /// Returns the transmission constraint for the current packet
    fn transmission_constraint(&self) -> transmission::Constraint;

    /// Returns the transmission mode for the current packet
    fn transmission_mode(&self) -> transmission::Mode;

    /// Returns the number of available bytes remaining in the current payload
    fn remaining_capacity(&self) -> usize;

    /// Attempt to write an ack frame.
    ///
    /// If this was successful the number of the packet
    /// that will be used to send the frame will be returned.
    #[inline]
    fn write_ack_frame<AckRanges: AckRangesTrait>(
        &mut self,
        ack_frame: &Ack<AckRanges>,
    ) -> Option<PacketNumber> {
        self.write_frame(ack_frame)
    }

    /// Attempt to write a frame.
    ///
    /// If this was successful the number of the packet
    /// that will be used to send the frame will be returned.
    fn write_frame<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + FrameTrait,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>;

    /// Writes a pre-fitted frame.
    ///
    /// Callers should ensure the frame fits within the outgoing buffer when using this function.
    /// The context should panic if otherwise.
    fn write_fitted_frame<Frame>(&mut self, frame: &Frame) -> PacketNumber
    where
        Frame: EncoderValue + FrameTrait,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>;

    /// Attempt to write a frame, bypassing congestion controller constraint checks.
    /// If this was successful the number of the packet that will be used to send
    /// the frame will be returned.
    fn write_frame_forced<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + FrameTrait,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>;

    /// Returns the ack elicitation of the current packet
    fn ack_elicitation(&self) -> AckElicitation;

    /// Returns the packet number for the current packet
    fn packet_number(&self) -> PacketNumber;

    /// Returns the local endpoint type (client or server)
    fn local_endpoint_type(&self) -> endpoint::Type;

    /// Returns the length of the packet header in bytes
    fn header_len(&self) -> usize;

    /// Returns the length of the authentication tag in bytes
    fn tag_len(&self) -> usize;
}
