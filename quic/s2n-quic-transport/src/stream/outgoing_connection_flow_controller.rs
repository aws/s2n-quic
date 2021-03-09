// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use alloc::rc::Rc;
use core::cell::RefCell;
use s2n_quic_core::{frame::MaxData, varint::VarInt};

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
}

impl OutgoingConnectionFlowControllerImpl {
    pub fn new(initial_window_size: VarInt) -> Self {
        Self {
            total_available_window: initial_window_size,
            available_window: initial_window_size,
        }
    }

    pub fn acquire_window(&mut self, desired: VarInt) -> VarInt {
        let result = core::cmp::min(self.available_window, desired);
        self.available_window -= result;
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
}
