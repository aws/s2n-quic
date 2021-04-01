// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manages all timers inside a Connection

use crate::{
    connection::InternalConnectionId,
    timer::{TimerEntry, VirtualTimer},
};
use s2n_quic_core::time::Timestamp;

/// Holds a timer entry for a single connection
///
/// On a call to [`update()`] the single per-connection timer
/// instance will be updated if changed.
pub type ConnectionTimerEntry = TimerEntry<InternalConnectionId>;

/// Stores connection-level timer state
#[derive(Debug, Default)]
pub struct ConnectionTimers {
    /// The timer which is used to check peer idle times
    pub peer_idle_timer: VirtualTimer,
    /// The timer which is used to send packets to the peer before the idle
    /// timeout expires
    pub local_idle_timer: VirtualTimer,
    /// The timer for removing an initial id mapping
    pub initial_id_expiration_timer: VirtualTimer,
}

impl ConnectionTimers {
    /// Returns an iterator of the currently armed timer timestamps
    pub fn iter(&self) -> impl Iterator<Item = &Timestamp> {
        core::iter::empty()
            .chain(self.local_idle_timer.iter())
            .chain(self.peer_idle_timer.iter())
            .chain(self.initial_id_expiration_timer.iter())
    }

    pub fn cancel(&mut self) {
        self.peer_idle_timer.cancel();
        self.local_idle_timer.cancel();
        self.initial_id_expiration_timer.cancel();
    }
}
