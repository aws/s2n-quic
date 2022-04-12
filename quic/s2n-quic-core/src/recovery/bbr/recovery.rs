// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{recovery::bbr::recovery::State::*, time::Timestamp};

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
    #[allow(dead_code)] // TODO: Remove when used
    #[inline]
    pub(crate) fn packet_conservation(&self) -> bool {
        matches!(self, Conservation(_, _))
    }

    /// True if currently in recovery (either Conservation or Growth)
    #[inline]
    pub(crate) fn in_recovery(&self) -> bool {
        *self != Recovered
    }

    /// True if a single packet may be transmitted despite a cwnd constraint
    #[inline]
    pub(crate) fn requires_fast_retransmission(&self) -> bool {
        matches!(
            self,
            Conservation(_, FastRetransmission::RequiresTransmission)
        )
    }

    /// Called when a packet is transmitted
    #[inline]
    pub(crate) fn on_packet_sent(&mut self) {
        if let Conservation(recovery_start_time, FastRetransmission::RequiresTransmission) = self {
            // A packet has been sent since we entered recovery (fast retransmission)
            // so flip the state back to idle.
            *self = Conservation(*recovery_start_time, FastRetransmission::Idle)
        }
    }

    /// Called on each ack
    ///
    /// Returns `true` if the ack caused recovery to be exited
    #[inline]
    pub(crate) fn on_ack(&mut self, round_start: bool, time_sent: Timestamp) -> bool {
        match self {
            // Check if this ack causes the controller to exit recovery
            Conservation(recovery_start_time, _) | Growth(recovery_start_time) => {
                if time_sent > *recovery_start_time {
                    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                    //# A recovery period ends and the sender enters congestion avoidance
                    //# when a packet sent during the recovery period is acknowledged.
                    *self = Recovered;
                    return true;
                }
            }
            Recovered => {}
        }

        // Still in recovery, but if this is a new round we move from Conservation to Growth
        if let (Conservation(recovery_start_time, _), true) = (&self, round_start) {
            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
            //# After one round-trip in Fast Recovery:
            //#    BBR.packet_conservation = false
            *self = Growth(*recovery_start_time)
        }

        false
    }

    /// Called when a congestion event occurs (packet loss or ECN CE count increase)
    #[inline]
    pub(crate) fn on_congestion_event(&mut self, now: Timestamp) {
        match self {
            Recovered => {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                //# If the congestion window is reduced immediately, a
                //# single packet can be sent prior to reduction.  This speeds up loss
                //# recovery if the data in the lost packet is retransmitted and is
                //# similar to TCP as described in Section 5 of [RFC6675].
                *self = Conservation(now, FastRetransmission::RequiresTransmission);
            }
            Conservation(ref mut recovery_start_time, _) | Growth(ref mut recovery_start_time) => {
                // BBR only allows recovery to end when there has been no congestion in a round, so
                // extend the recovery period when congestion occurs while in recovery
                *recovery_start_time = now
            }
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
        let state = Recovered;

        assert!(!state.in_recovery());
        assert!(!state.packet_conservation());
        assert!(!state.requires_fast_retransmission());
    }

    #[test]
    fn conservation() {
        let now = NoopClock.get_time();
        let state = Conservation(now, FastRetransmission::RequiresTransmission);

        assert!(state.in_recovery());
        assert!(state.packet_conservation());
        assert!(state.requires_fast_retransmission());
    }

    #[test]
    fn growth() {
        let now = NoopClock.get_time();
        let state = Growth(now);

        assert!(state.in_recovery());
        assert!(!state.packet_conservation());
        assert!(!state.requires_fast_retransmission());
    }

    #[test]
    fn state_transitions() {
        let now = NoopClock.get_time();
        let mut state = Recovered;

        // Acking a packet while Recovered does not change the state
        assert!(!state.on_ack(true, now));
        assert_eq!(state, Recovered);

        // Congestion event moves Recovered to Conservation
        state.on_congestion_event(now);
        assert_eq!(
            state,
            Conservation(now, FastRetransmission::RequiresTransmission)
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
        state.on_congestion_event(now);
        assert_eq!(state, Conservation(now, FastRetransmission::Idle));

        // Ack received that starts a new round moves Conservation to Growth
        assert!(!state.on_ack(true, now));
        assert_eq!(state, Growth(now));

        // Congestion moves the recovery start time forward
        let now = now + Duration::from_secs(10);
        state.on_congestion_event(now);
        assert_eq!(state, Growth(now));

        // Ack for a packet sent before the recovery start time does not exit recovery
        let sent_time = now - Duration::from_secs(1);
        assert!(!state.on_ack(true, sent_time));
        assert_eq!(state, Growth(now));

        // Ack for a packet sent after the recovery start time exits recovery
        let sent_time = now + Duration::from_secs(1);
        assert!(state.on_ack(true, sent_time));
        assert_eq!(state, Recovered);

        // Ack for a packet sent after the recovery start time exits recovery even if in Conservation
        let mut state = Conservation(now, FastRetransmission::RequiresTransmission);
        assert!(state.on_ack(true, sent_time));
        assert_eq!(state, Recovered);
    }
}
