//! Tools for synchronizing data between peers

use crate::contexts::WriteContext;
use s2n_quic_core::{packet::number::PacketNumber, stream::StreamId, time::Timestamp};

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
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum DeliveryState<T> {
    /// The delivery of the information has not yet been requested
    NotRequested,
    /// The delivery had been requested, but not yet started
    Requested(T),
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
    pub fn cancel(&mut self) {
        let old_state = core::mem::replace(self, DeliveryState::Cancelled(None));
        *self = match old_state {
            DeliveryState::NotRequested => DeliveryState::Cancelled(None),
            DeliveryState::Requested(value)
            | DeliveryState::Delivered(value)
            | DeliveryState::InFlight(InFlightDelivery { value, .. }) => {
                DeliveryState::Cancelled(Some(value))
            }
            state @ DeliveryState::Cancelled(_) => state,
        };
    }

    /// Returns `true` if the delivery of the value had been cancelled
    pub fn is_cancelled(&self) -> bool {
        if let DeliveryState::Cancelled(_) = self {
            true
        } else {
            false
        }
    }

    /// Returns `true` if the delivery of a value is requested
    pub fn is_requested(&self) -> bool {
        if let DeliveryState::Requested(_) = self {
            true
        } else {
            false
        }
    }

    /// Returns `true` if the delivery is current in progress.
    /// A packet has been sent, but no acknowledgement has been retrieved so far.
    pub fn is_inflight(&self) -> bool {
        if let DeliveryState::InFlight(_) = self {
            true
        } else {
            false
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

mod data_sender;
pub use data_sender::{
    ChunkToFrameWriter, DataSender, DataSenderState, OutgoingDataFlowController,
};

mod once_sync;
pub use once_sync::OnceSync;
mod incremental_value_sync;
pub use incremental_value_sync::IncrementalValueSync;
