//! Synchronizes a value of type `T` exactly once towards the remote peer.

use crate::{
    contexts::{OnTransmitError, WriteContext},
    sync::{DeliveryState, InFlightDelivery, InflightPacketInfo, ValueToFrameWriter},
    transmission,
};
use s2n_quic_core::{ack, stream::StreamId};

/// Synchronizes a value of type `T` exactly once towards the remote peer.
///
/// Retransmissions of the value will be performed if it got lost. However it is
/// not possible to replace the value with a newer value.
///
/// `S` is of type [`ValueToFrameWriter`] and used to serialize the value
/// into an outgoing frame.
#[derive(Debug)]
pub struct OnceSync<T, S> {
    delivery: DeliveryState<T>,
    writer: S,
}

impl<T, S: Default> Default for OnceSync<T, S> {
    fn default() -> Self {
        Self {
            delivery: DeliveryState::NotRequested,
            writer: S::default(),
        }
    }
}

impl<T: Copy + Clone + Eq + PartialEq, S: ValueToFrameWriter<T>> OnceSync<T, S> {
    /// Creates a new OnceSync
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the payload had been delivered to the peer and had
    /// been acknowledged by the peer.
    pub fn is_delivered(&self) -> bool {
        self.delivery.is_delivered()
    }

    /// Returns `true` if the delivery is current in progress.
    /// A packet has been sent, but no acknowledgement has been retrieved so far.
    pub fn is_inflight(&self) -> bool {
        self.delivery.is_inflight()
    }

    /// Returns `true` if the synchronization has been cancelled
    pub fn is_cancelled(&self) -> bool {
        self.delivery.is_cancelled()
    }

    /// Requested delivery of the given value.
    pub fn request_delivery(&mut self, value: T) {
        if let DeliveryState::NotRequested = self.delivery {
            self.delivery = DeliveryState::Requested(value);
        }
    }

    /// Stop to synchronize the value to the peer
    pub fn stop_sync(&mut self) {
        self.delivery.cancel();
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        // If the packet containing the frame gets acknowledged, mark the delivery as
        // succeeded.
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.delivery = DeliveryState::Delivered(in_flight.value);
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        // If the packet containing the frame was lost, remove the in_flight information.
        // This will trigger resending it.
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.delivery = DeliveryState::Lost(in_flight.value);
            }
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(
        &mut self,
        stream_id: StreamId,
        context: &mut W,
    ) -> Result<(), OnTransmitError> {
        if let Some(value) = self
            .delivery
            .try_transmit(context.transmission_constraint())
            .cloned()
        {
            let packet_nr = self
                .writer
                .write_value_as_frame(value, stream_id, context)
                .ok_or(OnTransmitError::CouldNotWriteFrame)?;

            // Overwrite the information about the pending delivery
            self.delivery = DeliveryState::InFlight(InFlightDelivery {
                packet: InflightPacketInfo {
                    packet_nr,
                    timestamp: context.current_time(),
                },
                value,
            });
        }

        Ok(())
    }
}

impl<T, S> transmission::interest::Provider for OnceSync<T, S> {
    fn transmission_interest(&self) -> transmission::Interest {
        self.delivery.transmission_interest()
    }
}
