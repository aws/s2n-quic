// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines Context traits, which are passed to various lifecycle callbacks
//! within the connection in order to collect data

use crate::{connection::InternalConnectionId, transmission, wakeup_queue::WakeupHandle};

pub use transmission::Writer as WriteContext;

/// Enumerates error values for `on_transmit` calls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTransmitError {
    /// It was not possible to write a frame
    CouldNotWriteFrame,
    /// It was not possible to obtain a large enough space for writing a frame
    CouldNotAcquireEnoughSpace,
}

/// Enumerates error values for `on_transmit` calls on connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOnTransmitError {
    /// It was not possible to obtain a datagram to write into
    NoDatagram,
}

/// The context parameter which is passed from all external API calls
pub struct ConnectionApiCallContext<'a> {
    wakeup_handle: &'a WakeupHandle<InternalConnectionId>,
}

impl<'a> ConnectionApiCallContext<'a> {
    /// Creates an [`ConnectionApiCallContext`] from a [`WakeupHandle`]
    pub fn from_wakeup_handle(wakeup_handle: &'a WakeupHandle<InternalConnectionId>) -> Self {
        Self { wakeup_handle }
    }

    /// Returns a reference to the WakeupHandle
    pub fn wakeup_handle(&mut self) -> &WakeupHandle<InternalConnectionId> {
        self.wakeup_handle
    }
}

#[cfg(test)]
pub mod testing {
    pub use crate::transmission::writer::testing::{Writer as MockWriteContext, *};
}
