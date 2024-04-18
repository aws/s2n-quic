// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{ack, packet::number::PacketNumber};
use core::ops::RangeInclusive;

pub type AckRange = RangeInclusive<PacketNumber>;

#[derive(Clone, Debug, Default)]
pub struct Set {
    /// A stable ack-eliciting transmission
    ///
    /// In this case, "stable" means the oldest transmission that
    /// hasn't been acked by the peer.
    ///
    /// This transmission is stored to ensure the packet number ranges
    /// are always either removed or declared lost. Without it,
    /// the TX packet number would be a moving target
    /// and packet number ranges would never be removed.
    stable: Option<Transmission>,

    /// The latest ack-eliciting transmission
    latest: Option<Transmission>,
}

impl Set {
    /// Called when an ACK frame is bundled with an ack eliciting packet
    #[inline]
    pub fn on_transmit(&mut self, transmission: Transmission) {
        self.latest = Some(transmission);

        // only set the stable transmission if it's not set
        if self.stable.is_none() {
            self.stable = Some(transmission);
        }
    }

    /// Called when a set of packets was acknowledged or lost
    #[inline]
    pub fn on_update<A: ack::Set>(&mut self, ack_set: &A) -> Option<AckRange> {
        if let Some(ack_range) = self
            .latest
            .as_ref()
            .and_then(|transmission| transmission.ack_range(ack_set))
        {
            // the latest was either ACKed or lost so there is nothing pending
            self.stable = None;
            self.latest = None;
            return Some(ack_range);
        }

        if let Some(ack_range) = self
            .stable
            .as_ref()
            .and_then(|transmission| transmission.ack_range(ack_set))
        {
            // assign the latest transmission to stable
            self.stable = self.latest;
            return Some(ack_range);
        }

        None
    }

    /// Returns `true` if there are no pending ack-eliciting transmissions
    /// Only used by tests
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.stable.is_none() && self.latest.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct Transmission {
    pub sent_in_packet: PacketNumber,
    pub largest_received_packet_number_acked: PacketNumber,
}

impl Transmission {
    /// Called when a set of packets was acknowledged or lost
    #[inline]
    pub fn ack_range<A: ack::Set>(&self, ack_set: &A) -> Option<AckRange> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.2.4
        //# When a packet containing an ACK frame is acknowledged, the receiver can stop
        //# acknowledging packets less than or equal to the Largest Acknowledged
        //# field in the sent ACK frame.
        if ack_set.contains(self.sent_in_packet) {
            // create a range from 0..=largest
            let pn_zero = self
                .largest_received_packet_number_acked
                .space()
                .new_packet_number(Default::default());

            Some(pn_zero..=self.largest_received_packet_number_acked)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ack::testing::transmissions_iter;

    /// This test is meant to simulate an immediate ACK rate from a peer
    #[test]
    fn latest_ack_test() {
        let mut set = Set::default();
        assert!(set.is_empty());

        let mut transmissions = transmissions_iter();

        for transmission_count in 0..10 {
            let stable = transmissions.next().unwrap();
            set.on_transmit(stable);

            // simulate a few transmissions before ACKing
            let mut latest = stable;
            for _ in 0..transmission_count {
                latest = transmissions.next().unwrap();
                set.on_transmit(latest);
            }

            // ACKing `latest` simulates an immediate ACK from peer
            assert_eq!(
                set.on_update(&latest.sent_in_packet).unwrap().end(),
                &latest.largest_received_packet_number_acked
            );

            // ACKing the latest removes all items
            assert!(set.is_empty());
        }
    }

    /// This test is meant to simulate a delayed ACK rate from a peer
    #[test]
    fn stable_ack_test() {
        let mut set = Set::default();
        assert!(set.is_empty());

        let mut transmissions = transmissions_iter();

        // create an initial transmission
        let mut stable = transmissions.next().unwrap();
        set.on_transmit(stable);

        for transmission_count in 0..10 {
            // simulate a few transmissions before ACKing
            let mut latest = stable;
            for _ in 0..transmission_count {
                latest = transmissions.next().unwrap();
                set.on_transmit(latest);
            }

            // ACKing `stable` simulates a delayed ACK from peer
            assert_eq!(
                set.on_update(&stable.sent_in_packet).unwrap().end(),
                &stable.largest_received_packet_number_acked
            );

            // on update latest is moved into stable
            stable = latest;

            // if there weren't any transmissions between, the set
            // should be empty
            if transmission_count == 0 {
                assert!(set.is_empty());
                set.on_transmit(stable);
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // miri is unable to read the file system
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("Transmission", size_of::<Transmission>());
        assert_debug_snapshot!("Set", size_of::<Set>());
    }
}
