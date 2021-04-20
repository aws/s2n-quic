// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    sync::{PeriodicSync, ValueToFrameWriter},
    transmission,
    transmission::Interest,
};
use alloc::rc::Rc;
use core::{cell::RefCell, time::Duration};
use s2n_quic_core::{
    ack,
    frame::{DataBlocked, MaxData},
    packet::number::PacketNumber,
    stream::StreamId,
    time::Timestamp,
    varint::VarInt,
};

/// The actual implementation/state of the per Connection flow controller for
/// outgoing data
#[derive(Debug)]
struct OutgoingConnectionFlowControllerImpl {
    /// The total connection flow control window as indicated through
    /// transport parameters and `MAX_DATA` frames from the peer.
    total_available_window: VarInt,
    /// The flow control window which has not yet been handed out to `Stream`s
    /// for sending data.
    available_window: VarInt,
    /// For periodically sending `DATA_BLOCKED` frames when blocked by peer limits
    data_blocked_sync: PeriodicSync<VarInt, DataBlockedToFrameWriter>,
}

impl OutgoingConnectionFlowControllerImpl {
    pub fn new(initial_window_size: VarInt) -> Self {
        Self {
            total_available_window: initial_window_size,
            available_window: initial_window_size,
            data_blocked_sync: PeriodicSync::new(),
        }
    }

    pub fn acquire_window(&mut self, desired: VarInt) -> VarInt {
        let result = core::cmp::min(self.available_window, desired);
        self.available_window -= result;

        if result < desired {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.1
            //# A sender SHOULD send a
            //# STREAM_DATA_BLOCKED or DATA_BLOCKED frame to indicate to the receiver
            //# that it has data to write but is blocked by flow control limits.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.1
            //# To keep the
            //# connection from closing, a sender that is flow control limited SHOULD
            //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
            //# has no ack-eliciting packets in flight.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.12
            //# A sender SHOULD send a DATA_BLOCKED frame (type=0x14) when it wishes
            //# to send data, but is unable to do so due to connection-level flow
            //# control; see Section 4.
            self.data_blocked_sync
                .request_delivery(self.total_available_window);
        }

        result
    }

    pub fn on_max_data(&mut self, frame: MaxData) {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.1
        //# A sender MUST ignore any MAX_STREAM_DATA or MAX_DATA frames that do
        //# not increase flow control limits.
        if self.total_available_window >= frame.maximum_data {
            return;
        }

        let increment = frame.maximum_data - self.total_available_window;
        self.total_available_window = frame.maximum_data;
        self.available_window += increment;

        // We now have more capacity from the peer so stop sending DATA_BLOCKED frames
        self.data_blocked_sync.stop_sync();
    }
}

/// Writes the `DATA_BLOCKED` frames.
#[derive(Debug, Default)]
pub(super) struct DataBlockedToFrameWriter {}

impl ValueToFrameWriter<VarInt> for DataBlockedToFrameWriter {
    fn write_value_as_frame<W: WriteContext>(
        &self,
        value: VarInt,
        _stream_id: StreamId,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&DataBlocked { data_limit: value })
    }
}

/// Manages the flow control window for sending data to peers.
///
/// The FlowController tracks the total flow control budget,
/// and will hand out parts of it to Streams if they intend to send data.
#[derive(Clone, Debug)]
pub struct OutgoingConnectionFlowController {
    inner: Rc<RefCell<OutgoingConnectionFlowControllerImpl>>,
}

impl OutgoingConnectionFlowController {
    /// Creates a new `OutgoingConnectionFlowController`
    pub fn new(initial_window_size: VarInt) -> Self {
        Self {
            inner: Rc::new(RefCell::new(OutgoingConnectionFlowControllerImpl::new(
                initial_window_size,
            ))),
        }
    }

    /// Returns the total connection flow control window as indicated through
    /// transport parameters and `MAX_DATA` frames from the peer.
    pub fn total_window(&self) -> VarInt {
        self.inner.borrow().total_available_window
    }

    /// Returns the flow control window which is still available for acquiring
    pub fn available_window(&self) -> VarInt {
        self.inner.borrow().available_window
    }

    /// Acquires a part of the window from the `ConnectionFlowController` in
    /// order to be able to use it for sending data. `desired` is the window
    /// size that is intended to be borrowed. The returned window size might
    /// be smaller if only a smaller window is available.
    ///
    /// The requested and returned window sizes are relative window sizes and
    /// do not refer to a particular offset in the reported MAX_DATA values.
    pub fn acquire_window(&mut self, desired: VarInt) -> VarInt {
        self.inner.borrow_mut().acquire_window(desired)
    }

    /// This method should be called when a `MAX_DATA` frame is received,
    /// which signals an increase in the available flow control budget.
    pub fn on_max_data(&mut self, frame: MaxData) {
        self.inner.borrow_mut().on_max_data(frame)
    }

    /// This method is called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.inner
            .borrow_mut()
            .data_blocked_sync
            .on_packet_ack(ack_set)
    }

    /// This method is called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.inner
            .borrow_mut()
            .data_blocked_sync
            .on_packet_loss(ack_set);
    }

    /// Updates the period at which `DATA_BLOCKED` frames are sent to the peer
    /// if the application is blocked by peer limits.
    pub fn update_blocked_sync_period(&mut self, blocked_sync_period: Duration) {
        self.inner
            .borrow_mut()
            .data_blocked_sync
            .update_sync_period(blocked_sync_period);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4.1
        //# To keep the
        //# connection from closing, a sender that is flow control limited SHOULD
        //# periodically send a STREAM_DATA_BLOCKED or DATA_BLOCKED frame when it
        //# has no ack-eliciting packets in flight.
        let data_blocked_sync = &mut self.inner.borrow_mut().data_blocked_sync;

        if context.ack_elicitation().is_ack_eliciting() && data_blocked_sync.has_delivered() {
            // We are already sending an ack-eliciting packet, so no need to send another DATA_BLOCKED
            data_blocked_sync.skip_delivery(context.current_time());
            Ok(())
        } else {
            // Stream ID does not matter here, since it does not get transmitted
            data_blocked_sync.on_transmit(StreamId::from_varint(VarInt::from_u32(0)), context)
        }
    }

    /// Returns all timers for the component
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.inner.borrow().data_blocked_sync.timers()
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        self.inner.borrow_mut().data_blocked_sync.on_timeout(now)
    }
}

/// Queries the component for interest in transmitting frames
impl transmission::interest::Provider for OutgoingConnectionFlowController {
    fn transmission_interest(&self) -> Interest {
        self.inner
            .borrow()
            .data_blocked_sync
            .transmission_interest()
    }
}
