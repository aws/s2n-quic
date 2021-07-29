// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Synchronizes a strictly increasing value of type `T` towards the remote peer.

use crate::{
    contexts::{OnTransmitError, WriteContext},
    sync::{DeliveryState, InFlightDelivery, InflightPacketInfo, ValueToFrameWriter},
    transmission,
};
use s2n_quic_core::{ack, stream::StreamId};

/// Synchronizes a strictly increasing value of type `T` towards the remote peer.
///
/// IncrementalValueSync will only send an update if it is significant enough (above a
/// certain `threshold`), or if the last update had been lost.
/// `S` is of type [`ValueToFrameWriter`] and used to serialize the latest value
/// into an outgoing frame.
#[derive(Debug)]
pub struct IncrementalValueSync<T, S> {
    latest_value: T,
    value_ackd_up_to: T,
    threshold: T,
    delivery: DeliveryState<T>,
    writer: S,
}

impl<
        T: Copy + Clone + core::fmt::Debug + Eq + PartialEq + PartialOrd + core::ops::Sub<Output = T>,
        S: ValueToFrameWriter<T>,
    > IncrementalValueSync<T, S>
{
    pub fn new(latest_value: T, value_ackd_up_to: T, threshold: T) -> Self {
        debug_assert!(
            latest_value >= value_ackd_up_to,
            "value to sync must be bigger or equal than the last acknowledged value"
        );
        let mut sync = IncrementalValueSync {
            latest_value,
            value_ackd_up_to,
            delivery: DeliveryState::NotRequested,
            threshold,
            writer: S::default(),
        };

        sync.request_delivery_if_necessary();

        sync
    }

    /// Returns the latest value that needs to get synchronized
    pub fn latest_value(&self) -> T {
        self.latest_value
    }

    /// Returns `true` if the synchronization has been cancelled
    pub fn is_cancelled(&self) -> bool {
        self.delivery.is_cancelled()
    }

    /// Returns `true` if the delivery is current in progress.
    /// A packet has been sent, but no acknowledgement has been retrieved so far.
    pub fn is_inflight(&self) -> bool {
        self.delivery.is_inflight()
    }

    /// Sets the new value that needs to get synchronized to the peer.
    /// Returns true if new value requires `on_transmit` to be called as soon as
    /// possible.
    pub fn update_latest_value(&mut self, value: T) {
        debug_assert!(value >= self.latest_value);
        self.latest_value = value;
        self.request_delivery_if_necessary();
    }

    /// Stop to synchronize the value to the peer
    pub fn stop_sync(&mut self) {
        self.delivery.cancel();
    }

    /// If the latest value is high enough to require sending an update, this
    /// sets the delivery state to `Requested`.
    fn request_delivery_if_necessary(&mut self) {
        if self.should_send_update() {
            self.delivery = DeliveryState::Requested(self.latest_value);
        }
    }

    /// Returns whether an update about the state of the stored value should be
    /// sent to the peer.
    ///
    /// Updates are only sent if the value that needs to get synchronized to the
    /// peer exceeds the value acknowledged by the peer by the configured `threshold`.
    fn should_send_update(&self) -> bool {
        if self.delivery.is_cancelled() {
            return false;
        }

        // Check if the window has already been fully transmitted and ackd
        if self.latest_value != self.value_ackd_up_to {
            // Check if a frame is already in transmission
            if let DeliveryState::InFlight(in_flight) = self.delivery {
                // Check if the new update is significant enough to supersede the old one
                // or whether we think the old update had been lost.
                if self.latest_value - in_flight.value >= self.threshold {
                    // TODO: Using the 10% threshold means we will overwrite the
                    // tracking information about older in-flight packets all the
                    // time, and only adjust `value_ackd_up_to` if the latest
                    // packet gets acknowledged. That might lead us in some cases
                    // to send a re-transmit even when not strictly necessary, since
                    // we don't observe a previous transmit being acknowledged.
                    // Either tracking more pending transmissions, or increasing
                    // the threshold could improve on that.
                    return true;
                }
            } else {
                // Check if the new update is significant enough
                if self.latest_value - self.value_ackd_up_to >= self.threshold {
                    return true;
                }
            }
        }

        false
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        // If the frame gets acknowledged, remove the in_flight information
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.value_ackd_up_to = in_flight.value;
                self.delivery = DeliveryState::NotRequested;
                // There is no need to call request_delivery_if_necessary() here:
                // If the value would have been updated significantly enough the
                // update would already have been superseeded.
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        // If the frame was lost, remove the in_flight information.
        // This will trigger resending it.
        if let DeliveryState::InFlight(in_flight) = self.delivery {
            if ack_set.contains(in_flight.packet.packet_nr) {
                self.delivery = DeliveryState::Lost(self.latest_value);
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
            // We grab the latest value here, even if an older one had been
            // requested for delivery. That makes sure we always transmit the
            // highest available value to the peer.
            let value = self.latest_value;

            let packet_nr = self
                .writer
                .write_value_as_frame(value, stream_id, context)
                .ok_or(OnTransmitError::CouldNotWriteFrame)?;

            // Overwrite the information about the in_flight transmission of the
            // latest value.
            self.delivery = DeliveryState::InFlight(InFlightDelivery {
                value,
                packet: InflightPacketInfo {
                    packet_nr,
                    timestamp: context.current_time(),
                },
            });
        }

        Ok(())
    }
}

impl<T, S> transmission::interest::Provider for IncrementalValueSync<T, S> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.delivery.transmission_interest(query)
    }
}
