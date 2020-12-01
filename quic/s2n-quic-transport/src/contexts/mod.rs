//! Defines Context traits, which are passed to various lifecycle callbacks
//! within the connection in order to collect data

use crate::{connection::InternalConnectionId, transmission, wakeup_queue::WakeupHandle};
use s2n_codec::encoder::EncoderValue;
use s2n_quic_core::{
    endpoint,
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
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

    /// Attempt to write a frame. If this was successful the number of the packet
    /// that will be used to send the frame will be returned.
    fn write_frame<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber>;

    /// Returns the ack elicitation of the current packet
    fn ack_elicitation(&self) -> AckElicitation;

    /// Returns the packet number for the current packet
    fn packet_number(&self) -> PacketNumber;

    /// Reserves a minimum amount of space for writing a frame. If the reservation
    /// fails an an error will be returned. If the reserveation succeeds, the
    /// method will return the actual available space for writing a frame in
    /// the `Ok` variant.
    fn reserve_minimum_space_for_frame(&mut self, min_size: usize) -> Result<usize, ()>;

    /// Returns the local endpoint type (client or server)
    fn local_endpoint_type(&self) -> endpoint::Type;
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
