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
///
/// The BBR definition of "Fast Recovery" differs from congestion controllers such
/// as Cubic in two ways:
///
/// 1) Fast recovery consists of two phases with differing impact on the rate at which
///    the congestion window may grow. After one round in the "Conservation" phase, in which
///    congestion window growth is limited to newly acked bytes, the "Growth" phase is entered,
///    in which the congestion window growth is limited to no more than twice the current
///    delivery rate.
/// 2) Recovery ends when there are no further losses in a round. This is not defined in the BBRv2
///    draft RFC (yet), but is mentioned in the Chromium source here:
///         <https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quic/core/congestion_control/bbr_sender.cc;drc=401f9911c6a32a0900f3968258393a9e729da625;l=696>
///    This differs from the QUIC RFC 9002 definition that states: "A recovery period ends and the
///    sender enters congestion avoidance when a packet sent during the recovery period is
///    acknowledged."
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Not currently in recovery
    Recovered,
    /// Using packet conservation dynamics to bound cwnd
    ///
    /// If a packet sent after the `Timestamp` is acknowledged, recovery is exited
    /// `FastRetransmission` allows for one packet to be transmitted after entering recovery.
    Conservation(Timestamp, FastRetransmission),
    /// Still in recovery, but allowing cwnd to grow at a higher rate
    /// If a packet sent after the `Timestamp` is acknowledged, recovery is exited
    Growth(Timestamp),
}

impl State {
    /// True if packet conservation dynamics should be used to bound cwnd
    #[inline]
    pub fn packet_conservation(&self) -> bool {
        matches!(self, State::Conservation(_, _))
    }

    /// True if currently in recovery (either Conservation or Growth)
    #[inline]
    pub fn in_recovery(&self) -> bool {
        *self != State::Recovered
    }

    /// True if a single packet may be transmitted despite a cwnd constraint
    #[inline]
    pub fn requires_fast_retransmission(&self) -> bool {
        matches!(
            self,
            State::Conservation(_, FastRetransmission::RequiresTransmission)
        )
    }

    /// Called when a packet is transmitted
    #[inline]
    pub fn on_packet_sent(&mut self) {
        if let State::Conservation(recovery_start_time, FastRetransmission::RequiresTransmission) =
            self
        {
            // A packet has been sent since we entered recovery (fast retransmission)
            // so flip the state back to idle.
            *self = State::Conservation(*recovery_start_time, FastRetransmission::Idle)
        }
    }

    /// Called on each ack
    ///
    /// Returns `true` if the ack caused recovery to be exited
    #[inline]
    pub fn on_ack(&mut self, round_start: bool, time_sent: Timestamp) -> bool {
        match self {
            // Check if this ack causes the controller to exit recovery
            State::Conservation(recovery_start_time, _) | State::Growth(recovery_start_time) => {
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

        // Still in recovery, but if this is a new round we move from Conservation to Growth
        if let (State::Conservation(recovery_start_time, _), true) = (&self, round_start) {
            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
            //# After one round-trip in Fast Recovery:
            //#    BBR.packet_conservation = false
            *self = State::Growth(*recovery_start_time)
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
                *self = State::Conservation(now, FastRetransmission::RequiresTransmission);
                true
            }
            State::Conservation(ref mut recovery_start_time, _)
            | State::Growth(ref mut recovery_start_time) => {
                // BBR only allows recovery to end when there has been no congestion in a round, so
                // extend the recovery period when congestion occurs while in recovery
                *recovery_start_time = now;
                false
            }
        }
    }

    #[inline]
    pub fn on_packet_discarded(&mut self) {
        if let State::Conservation(recovery_start_time, FastRetransmission::RequiresTransmission) =
            self
        {
            // If any of the discarded packets were lost, they will no longer be retransmitted
            // so flip the Recovery status back to Idle so it is not waiting for a
            // retransmission that may never come.
            *self = State::Conservation(*recovery_start_time, FastRetransmission::Idle)
        }
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
        assert!(!state.packet_conservation());
        assert!(!state.requires_fast_retransmission());
    }

    #[test]
    fn conservation() {
        let now = NoopClock.get_time();
        let state = State::Conservation(now, FastRetransmission::RequiresTransmission);

        assert!(state.in_recovery());
        assert!(state.packet_conservation());
        assert!(state.requires_fast_retransmission());
    }

    #[test]
    fn growth() {
        let now = NoopClock.get_time();
        let state = State::Growth(now);

        assert!(state.in_recovery());
        assert!(!state.packet_conservation());
        assert!(!state.requires_fast_retransmission());
    }

    #[test]
    fn state_transitions() {
        let now = NoopClock.get_time();
        let mut state = State::Recovered;

        // Acking a packet while Recovered does not change the state
        assert!(!state.on_ack(true, now));
        assert_eq!(state, State::Recovered);

        // Congestion event moves Recovered to Conservation
        assert!(state.on_congestion_event(now));
        assert_eq!(
            state,
            State::Conservation(now, FastRetransmission::RequiresTransmission)
        );
        assert!(state.requires_fast_retransmission());

        // Sending a packet moves FastRetransmission to Idle
        state.on_packet_sent();
        assert!(!state.requires_fast_retransmission());

        // Ack received in the same round does not change the state
        assert!(!state.on_ack(false, now));
        assert!(state.packet_conservation());

        // Congestion moves the recovery start time forward
        let now = now + Duration::from_secs(5);
        assert!(!state.on_congestion_event(now));
        assert_eq!(state, State::Conservation(now, FastRetransmission::Idle));

        // Ack received that starts a new round moves Conservation to Growth
        assert!(!state.on_ack(true, now));
        assert_eq!(state, State::Growth(now));

        // Congestion moves the recovery start time forward
        let now = now + Duration::from_secs(10);
        assert!(!state.on_congestion_event(now));
        assert_eq!(state, State::Growth(now));

        // Ack for a packet sent before the recovery start time does not exit recovery
        let sent_time = now - Duration::from_secs(1);
        assert!(!state.on_ack(true, sent_time));
        assert_eq!(state, State::Growth(now));

        // Ack for a packet sent after the recovery start time exits recovery
        let sent_time = now + Duration::from_secs(1);
        assert!(state.on_ack(true, sent_time));
        assert_eq!(state, State::Recovered);

        // Ack for a packet sent after the recovery start time exits recovery even if in Conservation
        let mut state = State::Conservation(now, FastRetransmission::RequiresTransmission);
        assert!(state.on_ack(true, sent_time));
        assert_eq!(state, State::Recovered);

        // Discarded packet sets FastRetransmission back to Idle
        let mut state = State::Conservation(now, FastRetransmission::RequiresTransmission);
        state.on_packet_discarded();
        assert_eq!(state, State::Conservation(now, FastRetransmission::Idle));
    }
}
