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
// in the `update_sync_period` method.
pub const DEFAULT_SYNC_PERIOD: Duration = Duration::from_secs(10);

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
    delivered: bool,
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
            delivered: false,
        }
    }

    /// Requested delivery of the given value. If delivery has already been requested, the
    /// original value will be overwritten. The new value must be greater than or equal
    /// to the original value.
    pub fn request_delivery(&mut self, value: T) {
        debug_assert!(value >= self.latest_value);

        self.latest_value = value;

        if matches!(
            self.delivery,
            DeliveryState::NotRequested | DeliveryState::Cancelled(_)
        ) {
            self.delivery = DeliveryState::Requested(value);
        }
    }

    /// Skip delivery for this sync period. A delivery will be scheduled for the next sync period.
    pub fn skip_delivery(&mut self, now: Timestamp) {
        match self.delivery {
            DeliveryState::Requested(_) | DeliveryState::Lost(_) => {
                self.delivery = DeliveryState::NotRequested;
                self.delivery_timer.set(now + self.sync_period)
            }
            DeliveryState::Delivered(_) => self.delivery_timer.set(now + self.sync_period),
            _ => {}
        }
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.delivery_timer.poll_expiration(now).is_ready() {
            self.delivery = DeliveryState::Requested(self.latest_value);
        }
    }

    /// Returns the timer for a scheduled delivery
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.delivery_timer.iter()
    }

    /// Stop synchronizing the value to the peer
    pub fn stop_sync(&mut self) {
        self.delivery_timer.cancel();
        self.delivery.cancel();
        self.delivered = false;
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        // If the packet containing the frame gets acknowledged, schedule a delivery for the
        // next delivery period
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.delivered = true;
                self.delivery_timer
                    .set(in_flight.packet.timestamp + self.sync_period);
                self.delivery = DeliveryState::Delivered(in_flight.value);
            }
        }
    }

    /// Sets the sync period. The given `sync_period` will be used the next time
    /// the delivery timer is armed; the existing timer will be unaffected.
    pub fn update_sync_period(&mut self, sync_period: Duration) {
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
    #[inline]
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

    /// Returns whether the value has been delivered at least once since delivery was first
    /// requested, or requested again after `stop_sync` was called.
    #[inline]
    pub fn has_delivered(&self) -> bool {
        self.delivered
    }
}

impl<T, S> transmission::interest::Provider for PeriodicSync<T, S> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.delivery.transmission_interest(query)
    }
}
