// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{space::rx_packet_numbers::ack_ranges::AckRanges, transmission};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AckTransmissionState {
    /// No ACK frames will be transmitted
    Disabled,

    /// An ACK frame may be transmitted, but isn't required. While in this
    /// state, the ACK Manager won't express interest in transmission, but
    /// will transmit ACK frames if other components are transmitting.
    Passive {
        /// The number of remaining tranmissions for the current ack ranges
        retransmissions: usize,
    },

    /// An ACK frame is required to be transmitted. While in this state,
    /// the ACK Manager will express interest in transmission.
    Active {
        /// The number of remaining tranmissions for the current ack ranges
        retransmissions: usize,
    },
}

impl Default for AckTransmissionState {
    fn default() -> Self {
        AckTransmissionState::Disabled
    }
}

impl AckTransmissionState {
    /// Returns `true` if the state is set to `Active`
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    /// Returns `true` if ACK frames should be transmitted, either actively or passively
    pub fn should_transmit(&self, constraint: transmission::Constraint) -> bool {
        match self {
            Self::Disabled => false,
            // Only transmit acks in Passive mode if we can transmit other frames as well
            Self::Passive { .. } => constraint.can_transmit() || constraint.can_retransmit(),
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7
            //# packets containing only ACK frames do not count
            //# towards bytes in flight and are not congestion controlled.
            Self::Active { .. } => true,
        }
    }

    /// Transitions the transmission to active if there are pending retransmissions
    pub fn activate(&mut self) -> &mut Self {
        if let Self::Passive { retransmissions } = *self {
            *self = AckTransmissionState::Active { retransmissions }
        }

        self
    }

    /// Notify the transmission state that pending ack ranges has updated
    pub fn on_update(&mut self, ack_ranges: &AckRanges) -> &mut Self {
        // no need to transmit anything now
        if ack_ranges.is_empty() {
            *self = AckTransmissionState::Disabled;
            return self;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.2
        //# In general, frequent feedback from a receiver improves loss and
        //# congestion response, but this has to be balanced against excessive
        //# load generated by a receiver that sends an ACK frame in response to
        //# every ack-eliciting packet.

        // By default, ACK ranges are not retransmitted
        let mut new_retransmissions = 0;

        // A high `interval_len` indicates more RX packet reordering and/or loss
        //
        // Increment retransmissions for every N intervals we are ACKing
        //
        // TODO: 2 was chosen as an initial hunch - we need to do some experiements to improve
        const INTERVAL_SCALE: usize = 2;
        new_retransmissions += ack_ranges.interval_len() / INTERVAL_SCALE;

        // A high count indicates more outstanding RX packets to be ACKed, which
        // results in higher risk of the peer retransmitting packet data which has
        // already been processed.
        //
        // Increment retransmissions for every N packets we are ACKing to increase
        // the likelyhood of ACK frames being received by the peer.
        //
        // TODO: 10 was chosen as an initial hunch - we need to do some experiements to improve
        const RANGE_SCALE: usize = 10;
        new_retransmissions += ack_ranges.spread() / RANGE_SCALE;

        match self {
            Self::Active { retransmissions } => {
                *retransmissions = new_retransmissions;
            }
            Self::Passive { retransmissions } => {
                *retransmissions = new_retransmissions;
            }
            Self::Disabled => {
                *self = AckTransmissionState::Passive {
                    retransmissions: new_retransmissions,
                };
            }
        }

        self
    }

    /// Notifies the transmission state that a transmission occurred
    pub fn on_transmit(&mut self) -> &mut Self {
        match *self {
            Self::Active { retransmissions } | Self::Passive { retransmissions } => {
                if let Some(retransmissions) = retransmissions.checked_sub(1) {
                    *self = AckTransmissionState::Passive { retransmissions };
                } else {
                    *self = AckTransmissionState::Disabled;
                }
            }
            Self::Disabled => {
                debug_assert!(false, "ACKs should not be transmitted in `Disabled` state");
            }
        }

        self
    }
}

impl transmission::interest::Provider for AckTransmissionState {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if self.is_active() {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7
            //# packets containing only ACK frames do not count
            //# towards bytes in flight and are not congestion controlled
            query.on_forced()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{super::tests::packet_numbers_iter, *};

    #[test]
    fn should_transmit_test() {
        for constraint in &[
            transmission::Constraint::None,
            transmission::Constraint::AmplificationLimited,
            transmission::Constraint::CongestionLimited,
            transmission::Constraint::RetransmissionOnly,
        ] {
            assert!(
                !AckTransmissionState::Disabled.should_transmit(*constraint),
                "disabled state should not transmit"
            );

            if constraint.can_transmit() || constraint.can_retransmit() {
                assert!(
                    AckTransmissionState::Passive { retransmissions: 1 }
                        .should_transmit(*constraint),
                    "passive state should transmit if not constrained"
                );
            } else {
                assert!(
                    !AckTransmissionState::Passive { retransmissions: 1 }
                        .should_transmit(*constraint),
                    "passive state should not transmit if constrained"
                );
            }

            assert!(
                AckTransmissionState::Active { retransmissions: 1 }.should_transmit(*constraint),
                "active state should transmit"
            );
        }
    }

    #[test]
    fn activate_test() {
        assert!(
            !AckTransmissionState::Disabled.activate().is_active(),
            "disabled state should not activate"
        );
        assert!(
            AckTransmissionState::Passive { retransmissions: 1 }
                .activate()
                .is_active(),
            "passive state should activate"
        );
        assert!(
            AckTransmissionState::Active { retransmissions: 1 }
                .activate()
                .is_active(),
            "active state should activate"
        );
    }

    #[test]
    #[should_panic]
    fn disabled_transmission_test() {
        AckTransmissionState::Disabled.on_transmit();
    }

    #[test]
    fn transmission_test() {
        assert_eq!(
            *AckTransmissionState::Passive { retransmissions: 0 }.on_transmit(),
            AckTransmissionState::Disabled,
            "transmitting should transition to Disabled"
        );

        assert_eq!(
            *AckTransmissionState::Passive { retransmissions: 1 }.on_transmit(),
            AckTransmissionState::Passive { retransmissions: 0 },
            "transmitting should decrement and stay in the same state"
        );
    }

    #[test]
    fn update_test() {
        let mut ack_ranges = AckRanges::new(10);
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number

        assert_eq!(
            *AckTransmissionState::Passive { retransmissions: 1 }.on_update(&ack_ranges),
            AckTransmissionState::Disabled,
            "empty ack_ranges should transition to Disabled"
        );

        ack_ranges.insert_packet_number(packet_numbers.next().unwrap());

        assert_eq!(
            *AckTransmissionState::Disabled.on_update(&ack_ranges),
            AckTransmissionState::Passive { retransmissions: 0 },
            "one ack range should transition to passive"
        );

        ack_ranges.insert_packet_number(packet_numbers.next().unwrap());
        ack_ranges.insert_packet_number(packet_numbers.next().unwrap());

        assert_eq!(
            *AckTransmissionState::Disabled.on_update(&ack_ranges),
            AckTransmissionState::Passive { retransmissions: 1 },
            "multiple ack ranges should transition to passive with retransmissions"
        );

        assert_eq!(
            *AckTransmissionState::Passive { retransmissions: 0 }.on_update(&ack_ranges),
            AckTransmissionState::Passive { retransmissions: 1 },
            "multiple ack ranges should update passive with retransmissions"
        );

        assert_eq!(
            *AckTransmissionState::Active { retransmissions: 0 }.on_update(&ack_ranges),
            AckTransmissionState::Active { retransmissions: 1 },
            "multiple ack ranges should update active with retransmissions"
        );
    }

    #[test]
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("AckTransmissionState", size_of::<AckTransmissionState>());
    }
}
