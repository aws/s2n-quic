// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
//# If the congestion window is reduced immediately, a
//# single packet can be sent prior to reduction.  This speeds up loss
//# recovery if the data in the lost packet is retransmitted and is
//# similar to TCP as described in Section 5 of [RFC6675].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FastRetransmission {
    Idle,
    RequiresTransmission,
}

/// Tracks the state of recovery for BBR
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Not currently in recovery
    Recovered,
    /// Recovering
    ///
    /// If a packet sent after the `Timestamp` is acknowledged, recovery is exited
    /// `FastRetransmission` allows for one packet to be transmitted after entering recovery.
    Recovering(Timestamp, FastRetransmission),
}

impl State {
    /// True if a single packet may be transmitted despite a cwnd constraint
    #[inline]
    pub fn requires_fast_retransmission(&self) -> bool {
        matches!(
            self,
            State::Recovering(_, FastRetransmission::RequiresTransmission)
        )
    }

    /// Called when a packet is transmitted
    #[inline]
    pub fn on_packet_sent(&mut self) {
        if let State::Recovering(_, transmission @ FastRetransmission::RequiresTransmission) = self
        {
            // A packet has been sent since we entered recovery (fast retransmission)
            // so flip the state back to idle.
            *transmission = FastRetransmission::Idle;
        }
    }

    /// Called on each ack
    ///
    /// Returns `true` if the ack caused recovery to be exited
    #[inline]
    pub fn on_ack(&mut self, time_sent: Timestamp) -> bool {
        match self {
            // Check if this ack causes the controller to exit recovery
            State::Recovering(recovery_start_time, _) => {
                if time_sent > *recovery_start_time {
                    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                    //# A recovery period ends and the sender enters congestion avoidance
                    //# when a packet sent during the recovery period is acknowledged.
                    *self = State::Recovered;
                    return true;
                }
            }
            State::Recovered => {}
        }

        false
    }

    /// Called when a congestion event occurs (packet loss or ECN CE count increase)
    ///
    /// Returns `true` if the congestion event caused recovery to be entered
    #[inline]
    pub fn on_congestion_event(&mut self, now: Timestamp) -> bool {
        match self {
            State::Recovered => {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                //# If the congestion window is reduced immediately, a
                //# single packet can be sent prior to reduction.  This speeds up loss
                //# recovery if the data in the lost packet is retransmitted and is
                //# similar to TCP as described in Section 5 of [RFC6675].
                *self = State::Recovering(now, FastRetransmission::RequiresTransmission);
                true
            }
            _ => false,
        }
    }

    #[inline]
    pub fn on_packet_discarded(&mut self) {
        if let State::Recovering(_, transmission @ FastRetransmission::RequiresTransmission) = self
        {
            // If any of the discarded packets were lost, they will no longer be retransmitted
            // so flip the Recovery status back to Idle so it is not waiting for a
            // retransmission that may never come.
            *transmission = FastRetransmission::Idle;
        }
    }

    /// True if currently in recovery
    #[cfg(test)]
    pub fn in_recovery(&self) -> bool {
        *self != State::Recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock as _, NoopClock};
    use std::time::Duration;

    #[test]
    fn recovered() {
        let state = State::Recovered;

        assert!(!state.in_recovery());
        assert!(!state.requires_fast_retransmission());
    }

    #[test]
    fn in_recovery() {
        let now = NoopClock.get_time();
        let state = State::Recovering(now, FastRetransmission::RequiresTransmission);

        assert!(state.in_recovery());
        assert!(state.requires_fast_retransmission());
    }

    #[test]
    fn state_transitions() {
        let now = NoopClock.get_time() + Duration::from_secs(10);
        let mut state = State::Recovered;

        // Acking a packet while Recovered does not change the state
        assert!(!state.on_ack(now));
        assert_eq!(state, State::Recovered);

        // Congestion event moves Recovered to InRecovery
        assert!(state.on_congestion_event(now));
        assert_eq!(
            state,
            State::Recovering(now, FastRetransmission::RequiresTransmission)
        );
        assert!(state.requires_fast_retransmission());

        // Sending a packet moves FastRetransmission to Idle
        state.on_packet_sent();
        assert!(!state.requires_fast_retransmission());

        // Ack for a packet sent before the recovery start time does not exit recovery
        let sent_time = now - Duration::from_secs(1);
        assert!(!state.on_ack(sent_time));
        assert_eq!(state, State::Recovering(now, FastRetransmission::Idle));

        // Ack for a packet sent after the recovery start time exits recovery
        let sent_time = now + Duration::from_secs(1);
        assert!(state.on_ack(sent_time));
        assert_eq!(state, State::Recovered);

        // Discarded packet sets FastRetransmission back to Idle
        let mut state = State::Recovering(now, FastRetransmission::RequiresTransmission);
        state.on_packet_discarded();
        assert_eq!(state, State::Recovering(now, FastRetransmission::Idle));
    }
}
