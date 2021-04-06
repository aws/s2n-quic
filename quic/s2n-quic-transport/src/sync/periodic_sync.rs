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

// The default period for synchronizing the value. This value is only used prior to a more
// precise value calculated based on idle timeout and current RTT estimates and provided
// in the `on_packet_ack` method.
const DEFAULT_SYNC_PERIOD: Duration = Duration::from_secs(10);

/// Synchronizes a monotonically increasing value of type `T` periodically towards the remote peer.
///
/// Retransmissions of the value will be performed if it got lost.
///
/// `S` is of type [`ValueToFrameWriter`] and used to serialize the value
/// into an outgoing frame.
#[derive(Debug)]
pub struct PeriodicSync<T, S> {
    latest_value: T,
    sync_period: Duration,
    delivery_timer: VirtualTimer,
    delivery: DeliveryState<T>,
    writer: S,
}

impl<T: Copy + Clone + Default + Eq + PartialEq + PartialOrd, S: ValueToFrameWriter<T>>
    PeriodicSync<T, S>
{
    /// Creates a new PeriodicSync. The value will transmitted when `request_delivery` is called
    /// and every subsequent `sync_period` until `stop_sync` is called.
    pub fn new() -> Self {
        Self {
            latest_value: T::default(),
            sync_period: DEFAULT_SYNC_PERIOD,
            delivery_timer: VirtualTimer::default(),
            delivery: DeliveryState::NotRequested,
            writer: S::default(),
        }
    }

    /// Requested delivery of the given value. If delivery has already been requested, the
    /// original value will be overwritten. The new value must be greater than or equal
    /// to the original value.
    pub fn request_delivery(&mut self, value: T) {
        debug_assert!(value >= self.latest_value);

        self.latest_value = value;

        if let DeliveryState::NotRequested = self.delivery {
            self.delivery = DeliveryState::Requested(value);
        }
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.delivery_timer.poll_expiration(now).is_ready() {
            self.delivery = DeliveryState::Requested(self.latest_value);
        }
    }

    /// Returns the timer for a scheduled delivery
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.delivery_timer.iter()
    }

    /// Stop synchronizing the value to the peer
    pub fn stop_sync(&mut self) {
        self.delivery_timer.cancel();
        self.delivery.cancel();
    }

    /// This method gets called when a packet delivery got acknowledged
    /// The given `sync_period` is used to update the period at which
    /// the value is synchronized with the peer.
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A, sync_period: Duration) {
        self.set_sync_period(sync_period);
        // If the packet containing the frame gets acknowledged, schedule a delivery for the
        // next delivery period
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.delivery_timer
                    .set(in_flight.packet.timestamp + self.sync_period);
                self.delivery = DeliveryState::Delivered(in_flight.value);
            }
        }
    }

    /// Sets the sync period. If a delivery is currently scheduled based on the existing
    /// sync period, the delivery time will be adjusted sooner or later based on the
    /// given `sync_period`
    fn set_sync_period(&mut self, sync_period: Duration) {
        if let Some(delivery_time) = self.delivery_timer.iter().next().copied() {
            if self.sync_period > sync_period {
                self.delivery_timer
                    .set(delivery_time - (self.sync_period - sync_period))
            } else {
                self.delivery_timer
                    .set(delivery_time + (sync_period - self.sync_period))
            }
        }

        self.sync_period = sync_period;
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
        if self
            .delivery
            .try_transmit(context.transmission_constraint())
            .is_some()
        {
            let value = self.latest_value;

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
