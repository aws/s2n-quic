// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines Context traits, which are passed to various lifecycle callbacks
//! within the connection in order to collect data

use crate::{connection::InternalConnectionId, transmission, wakeup_queue::WakeupHandle};
use s2n_codec::encoder::EncoderValue;
use s2n_quic_core::{
    endpoint,
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
        path_validation::Probing as PathValidationProbing,
    },
    packet::number::PacketNumber,
    time::Timestamp,
};

/// Context information that is passed to `on_transmit` calls on Streams
pub trait WriteContext {
    /// Returns the current point of time
    fn current_time(&self) -> Timestamp;

    /// Returns the transmission constraint for the current packet
    fn transmission_constraint(&self) -> transmission::Constraint;

    /// Returns the transmission mode for the current packet
    fn transmission_mode(&self) -> transmission::Mode;

    /// Returns the number of available bytes remaining in the current payload
    fn remaining_capacity(&self) -> usize;

    /// Attempt to write a frame.
    ///
    /// If this was successful the number of the packet
    /// that will be used to send the frame will be returned.
    fn write_frame<
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
    >(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber>;

    /// Writes a pre-fitted frame.
    ///
    /// Callers should ensure the frame fits within the outgoing buffer when using this function.
    /// The context should panic if otherwise.
    fn write_fitted_frame<
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
    >(
        &mut self,
        frame: &Frame,
    ) -> PacketNumber;

    /// Attempt to write a frame, bypassing congestion controller constraint checks.
    /// If this was successful the number of the packet that will be used to send
    /// the frame will be returned.
    fn write_frame_forced<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber>;

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

/// Enumerates error values for `on_transmit` calls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTransmitError {
    /// It was not possible to write a frame
    CouldNotWriteFrame,
    /// It was not possible to obtain a large enough space for writing a frame
    CoundNotAcquireEnoughSpace,
}

/// Enumerates error values for `on_transmit` calls on connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOnTransmitError {
    /// It was not possible to obtain a datagram to write into
    NoDatagram,
}

/// The context parameter which is passed from all external API calls
pub struct ConnectionApiCallContext<'a> {
    wakeup_handle: &'a mut WakeupHandle<InternalConnectionId>,
}

impl<'a> ConnectionApiCallContext<'a> {
    /// Creates an [`ConnectionApiCallContext`] from a [`WakeupHandle`]
    pub fn from_wakeup_handle(wakeup_handle: &'a mut WakeupHandle<InternalConnectionId>) -> Self {
        Self { wakeup_handle }
    }

    /// Returns a reference to the WakeupHandle
    pub fn wakeup_handle(&mut self) -> &mut WakeupHandle<InternalConnectionId> {
        &mut self.wakeup_handle
    }
}

#[cfg(test)]
pub mod testing;
