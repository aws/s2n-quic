// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Synchronizes a value of type `T` periodically towards the remote peer.

use crate::{
    contexts::{OnTransmitError, WriteContext},
    sync::{DeliveryState, InFlightDelivery, InflightPacketInfo, ValueToFrameWriter},
    timer::VirtualTimer,
    transmission,
};
use core::time::Duration;
use s2n_quic_core::{ack, stream::StreamId, time::Timestamp};

/// Synchronizes a value of type `T` periodically towards the remote peer.
///
/// Retransmissions of the value will be performed if it got lost.
///
/// `S` is of type [`ValueToFrameWriter`] and used to serialize the value
/// into an outgoing frame.
#[derive(Debug)]
pub struct PeriodicSync<T, S> {
    sync_period: Duration,
    delivery: DeliveryState<T>,
    writer: S,
}

impl<T: Copy + Clone + Eq + PartialEq, S: ValueToFrameWriter<T>> PeriodicSync<T, S> {
    /// Creates a new PeriodicSync. The value will transmitted once every `sync_period`
    pub fn new(sync_period: Duration) -> Self {
        Self {
            sync_period,
            delivery: DeliveryState::NotRequested,
            writer: S::default(),
        }
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

    /// Called when the pending delivery timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if let DeliveryState::Pending(mut delivery_timer, value) = self.delivery {
            if delivery_timer.poll_expiration(now).is_ready() {
                self.delivery = DeliveryState::Requested(value);
            }
        }
    }

    /// Returns the timer for a pending delivery
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        match &self.delivery {
            DeliveryState::Pending(delivery_timer, _) => delivery_timer.iter(),
            _ => None.iter(),
        }
    }

    /// Stop to synchronize the value to the peer
    pub fn stop_sync(&mut self) {
        self.delivery.cancel();
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        // If the packet containing the frame gets acknowledged, schedule a delivery for the
        // next delivery period
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                let mut next_delivery = VirtualTimer::default();
                next_delivery.set(in_flight.packet.timestamp + self.sync_period);
                self.delivery = DeliveryState::Pending(next_delivery, in_flight.value);
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

impl<T, S> transmission::interest::Provider for PeriodicSync<T, S> {
    fn transmission_interest(&self) -> transmission::Interest {
        self.delivery.transmission_interest()
    }
}
