// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tools for synchronizing data between peers

use crate::{contexts::WriteContext, transmission};
use s2n_quic_core::{packet::number::PacketNumber, stream::StreamId, time::Timestamp};

pub mod data_sender;
pub mod flag;
mod incremental_value_sync;
mod once_sync;
mod periodic_sync;

pub use incremental_value_sync::IncrementalValueSync;
pub use once_sync::OnceSync;
pub use periodic_sync::PeriodicSync;

#[cfg(test)]
pub use periodic_sync::DEFAULT_SYNC_PERIOD;

/// Carries information about the packet in which a frame is transmitted
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct InflightPacketInfo {
    /// The number of the packet that was used to send the message.
    packet_nr: PacketNumber,
    /// The timestamp when the message had been sent.
    timestamp: Timestamp,
}

/// A value delivery which is currently in progress
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InFlightDelivery<T> {
    /// The value which is sent
    value: T,
    /// The packet in which the value was sent
    packet: InflightPacketInfo,
}

/// Tracks the delivery of a particular piece of information to the peer.
#[derive(PartialEq, Eq, Debug)]
pub enum DeliveryState<T> {
    /// The delivery of the information has not yet been requested
    NotRequested,
    /// The delivery had been requested, but not yet started
    Requested(T),
    /// The original delivery was lost and needs to be retransmitted
    Lost(T),
    /// The delivery of the information has been requested and is in progress
    InFlight(InFlightDelivery<T>),
    /// The delivery of the information has succeeded
    Delivered(T),
    /// The delivery was cancelled. If a delivery of a value was previously
    /// requested it will be stored in the `Option`. Otherwise `None` will be
    /// stored.
    Cancelled(Option<T>),
}

impl<T> DeliveryState<T> {
    /// Moves the DeliverState into the `Cancelled` state.
    /// If a delivery was previously requested, the `Option` stored in
    /// `DeliverState::Cancelled` will contain the last value which was scheduled
    /// for delivery.
    #[inline]
    pub fn cancel(&mut self) {
        let old_state = core::mem::replace(self, DeliveryState::Cancelled(None));
        *self = match old_state {
            DeliveryState::NotRequested => DeliveryState::Cancelled(None),
            DeliveryState::Requested(value)
            | DeliveryState::Lost(value)
            | DeliveryState::Delivered(value)
            | DeliveryState::InFlight(InFlightDelivery { value, .. }) => {
                DeliveryState::Cancelled(Some(value))
            }
            state @ DeliveryState::Cancelled(_) => state,
        };
    }

    /// Returns `true` if the delivery of the value had been cancelled
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    /// Returns `true` if the delivery is current in progress.
    /// A packet has been sent, but no acknowledgement has been retrieved so far.
    #[inline]
    pub fn is_inflight(&self) -> bool {
        matches!(self, Self::InFlight(_))
    }

    /// Tries to transmit the delivery with the given transmission constraint
    #[inline]
    pub fn try_transmit(&self, constraint: transmission::Constraint) -> Option<&T> {
        match self {
            DeliveryState::Requested(value) if constraint.can_transmit() => Some(value),
            DeliveryState::Lost(value) if constraint.can_retransmit() => Some(value),
            _ => None,
        }
    }
}

impl<T> transmission::interest::Provider for DeliveryState<T> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match self {
            Self::Requested(_) => query.on_new_data(),
            Self::Lost(_) => query.on_lost_data(),
            _ => Ok(()),
        }
    }
}

/// Writes values of type `T` into frames.
pub trait ValueToFrameWriter<T>: Default {
    /// Creates a QUIC frame out of the given value, and writes it using the
    /// provided [`WriteContext`].
    /// The method returns the `PacketNumber` of the packet containing the value
    /// if the write was successful, and `None` otherwise.
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: T,
        stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber>;
}
