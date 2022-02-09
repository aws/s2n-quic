// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manages all timers inside a Connection

use s2n_quic_core::time::{timer, Timer};

/// Stores connection-level timer state
#[derive(Debug, Default)]
pub struct ConnectionTimers {
    /// The timer which is used to check peer idle times
    pub peer_idle_timer: Timer,
    /// Stores if sending an ack-eliciting packet will rearm the idle timer
    //= https://www.rfc-editor.org/rfc/rfc9000#10.1
    //# An endpoint also restarts its
    //# idle timer when sending an ack-eliciting packet if no other ack-
    //# eliciting packets have been sent since last receiving and processing
    //# a packet.
    pub reset_peer_idle_timer_on_send: bool,
    /// The timer which is used to send packets to the peer before the idle
    /// timeout expires
    pub local_idle_timer: Timer,
    /// The timer for removing an initial id mapping
    pub initial_id_expiration_timer: Timer,
    /// The timer for pacing transmission of packets
    pub pacing_timer: Timer,
    /// The timer for closing the connection if the handshake is still in progress
    pub max_handshake_duration_timer: Timer,
    /// The timer for calling the connection supervisor
    pub supervisor_timer: Timer,
}

impl ConnectionTimers {
    pub fn cancel(&mut self) {
        self.peer_idle_timer.cancel();
        self.local_idle_timer.cancel();
        self.initial_id_expiration_timer.cancel();
        self.pacing_timer.cancel();
        self.max_handshake_duration_timer.cancel();
        self.supervisor_timer.cancel();
    }
}

impl timer::Provider for ConnectionTimers {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.local_idle_timer.timers(query)?;
        self.peer_idle_timer.timers(query)?;
        self.initial_id_expiration_timer.timers(query)?;
        self.pacing_timer.timers(query)?;
        self.max_handshake_duration_timer.timers(query)?;
        self.supervisor_timer.timers(query)?;

        Ok(())
    }
}
