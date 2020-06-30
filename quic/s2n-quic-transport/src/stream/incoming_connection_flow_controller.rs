//! Manages the per-connecton flow-control window

use crate::{
    contexts::{OnTransmitError, WriteContext},
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    sync::{IncrementalValueSync, ValueToFrameWriter},
};
use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::{
    ack_set::AckSet, frame::max_data::MaxData, packet::number::PacketNumber, stream::StreamId,
    varint::VarInt,
};

/// Writes `MAX_DATA` frames based on the connections flow control window.
#[derive(Default, Debug)]
pub(super) struct MaxDataToFrameWriter {}

impl ValueToFrameWriter<VarInt> for MaxDataToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        _stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&MaxData {
            maximum_data: value,
        })
    }
}

/// The actual implementation/state of the per Connection flow controller for
/// incoming data
#[derive(Debug)]
struct IncomingConnectionFlowControllerImpl {
    /// Synchronizes the read window to the remote peer
    pub(super) read_window_sync: IncrementalValueSync<VarInt, MaxDataToFrameWriter>,
    /// The relative flow control window we want to maintain
    pub(super) desired_flow_control_window: u32,
    /// The amount of flow control credits which already have been acquired by
    /// Streams.
    pub(super) acquired_window: VarInt,
    /// The amount of flow control credits which had been acquired and where the
    /// data had already been consumed by the application
    pub(super) consumed_window: VarInt,
}

impl IncomingConnectionFlowControllerImpl {
    pub fn new(initial_window_size: VarInt, desired_flow_control_window: u32) -> Self {
        Self {
            read_window_sync: IncrementalValueSync::new(
                VarInt::from_u32(desired_flow_control_window),
                initial_window_size,
                VarInt::from_u32(desired_flow_control_window / 10),
            ),
            desired_flow_control_window,
            acquired_window: VarInt::from_u32(0),
            consumed_window: VarInt::from_u32(0),
        }
    }

    pub fn remaining_window(&self) -> VarInt {
        self.read_window_sync.latest_value() - self.acquired_window
    }

    #[cfg(test)]
    pub(super) fn current_receive_window(&self) -> VarInt {
        self.read_window_sync.latest_value()
    }

    // KCOV_END_TEST_MARKER

    pub fn release_window(&mut self, amount: VarInt) {
        self.consumed_window += amount;
        debug_assert!(
            self.consumed_window <= self.acquired_window,
            "Can not consume more window than previously acquired"
        );

        self.read_window_sync.update_latest_value(
            self.consumed_window
                .saturating_add(VarInt::from_u32(self.desired_flow_control_window)),
        );
    }

    pub fn acquire_window(&mut self, desired: VarInt) -> Result<(), ()> {
        if self.remaining_window() < desired {
            return Err(());
        }

        self.acquired_window += desired;
        Ok(())
    }

    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        self.read_window_sync.on_packet_ack(ack_set)
    }

    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        self.read_window_sync.on_packet_loss(ack_set)
    }

    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        // Stream ID does not matter here, since it does not get transmitted
        self.read_window_sync
            .on_transmit(StreamId::from_varint(VarInt::from_u32(0)), context)
    }
}

/// This component manages the flow control on the reception side.
///
/// It allows to check whether the peer sent more data on a per-connection base
/// then what was allowed through the flow control window.
///
/// It will also signal an increased window once data had been consumed.
#[derive(Clone, Debug)]
pub struct IncomingConnectionFlowController {
    inner: Rc<RefCell<IncomingConnectionFlowControllerImpl>>,
}

impl IncomingConnectionFlowController {
    /// Creates a new `IncomingConnectionFlowController`
    ///
    /// The connection flow controller will allow the peer to send up to
    /// `initial_window_size` bytes initially.
    ///
    /// The flow controller will try to maintain a window of
    /// `desired_flow_control_window`. This means if the window which is indicated
    /// to the peer is lower than this value the new value will be communicated
    /// to the peer.
    pub fn new(initial_window_size: VarInt, desired_flow_control_window: u32) -> Self {
        Self {
            inner: Rc::new(RefCell::new(IncomingConnectionFlowControllerImpl::new(
                initial_window_size,
                desired_flow_control_window,
            ))),
        }
    }

    /// Acquires a part of the window from the `IncomingConnectionFlowController` in
    /// in order to be able to use it for receiving data. `desired` is the window
    /// size that is intended to be borrowed.
    ///
    /// If the requested window size is not available the method will return
    /// an error in form of the `Err` variant.
    pub fn acquire_window(&mut self, desired: VarInt) -> Result<(), ()> {
        self.inner.borrow_mut().acquire_window(desired)
    }

    pub fn release_window(&mut self, amount: VarInt) {
        self.inner.borrow_mut().release_window(amount)
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        self.inner.borrow_mut().on_packet_ack(ack_set)
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        self.inner.borrow_mut().on_packet_loss(ack_set)
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        self.inner.borrow_mut().on_transmit(context)
    }

    #[cfg(test)]
    pub fn remaining_window(&self) -> VarInt {
        self.inner.borrow_mut().remaining_window()
    }

    // KCOV_END_TEST_MARKER

    /// Returns the MAX_DATA window that is currently synchronized
    /// towards the peer.
    #[cfg(test)]
    pub(super) fn current_receive_window(&self) -> VarInt {
        self.inner.borrow().current_receive_window()
    }

    // KCOV_END_TEST_MARKER

    #[cfg(test)]
    pub fn desired_flow_control_window(&self) -> u32 {
        self.inner.borrow().desired_flow_control_window
    }
    // KCOV_END_TEST_MARKER
}

impl FrameExchangeInterestProvider for IncomingConnectionFlowController {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        self.inner
            .borrow()
            .read_window_sync
            .frame_exchange_interests()
    }
}
