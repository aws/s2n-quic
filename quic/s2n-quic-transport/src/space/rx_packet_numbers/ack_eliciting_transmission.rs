use crate::frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests};
use core::ops::RangeInclusive;
use s2n_quic_core::{ack_set::AckSet, packet::number::PacketNumber};

pub type AckRange = RangeInclusive<PacketNumber>;

#[derive(Clone, Debug, Default)]
pub struct AckElicitingTransmissionSet {
    /// A stable ack-eliciting transmission
    ///
    /// In this case, "stable" means the oldest transmission that
    /// hasn't been acked by the peer.
    ///
    /// This transmission is stored to ensure the packet number ranges
    /// are always either removed or declared lost. Without it,
    /// the TX packet number would be a moving target
    /// and packet number ranges would never be removed.
    stable: Option<AckElicitingTransmission>,

    /// The latest ack-eliciting transmission
    latest: Option<AckElicitingTransmission>,
}

impl AckElicitingTransmissionSet {
    /// Called when an ACK frame is bundled with an ack eliciting packet
    pub fn on_transmit(&mut self, transmission: AckElicitingTransmission) {
        self.latest = Some(transmission);

        // only set the stable transmission if it's not set
        if self.stable.is_none() {
            self.stable = Some(transmission);
        }
    }

    /// Called when a set of packets was acknowledged or lost
    pub fn on_update<A: AckSet>(&mut self, ack_set: &A) -> Option<AckRange> {
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
    pub fn is_empty(&self) -> bool {
        self.stable.is_none() && self.latest.is_none()
    }
}

impl FrameExchangeInterestProvider for AckElicitingTransmissionSet {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        // only subscribe to notifications if there's pending transmissions
        let delivery_notifications = !self.is_empty();
        FrameExchangeInterests {
            delivery_notifications,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct AckElicitingTransmission {
    pub sent_in_packet: PacketNumber,
    pub largest_received_packet_number_acked: PacketNumber,
}

impl AckElicitingTransmission {
    /// Called when a set of packets was acknowledged or lost
    pub fn ack_range<A: AckSet>(&self, ack_set: &A) -> Option<AckRange> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#13.2.3
        //# When a packet containing an
        //# ACK frame is acknowledged, the receiver can stop acknowledging
        //# packets less than or equal to the largest acknowledged in the sent
        //# ACK frame.
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
    use super::{super::tests::transmissions_iter, *};

    /// This test is meant to simulate an immediate ACK rate from a peer
    #[test]
    fn latest_ack_test() {
        let mut set = AckElicitingTransmissionSet::default();
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
        let mut set = AckElicitingTransmissionSet::default();
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
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!(
            "AckElicitingTransmission",
            size_of::<AckElicitingTransmission>()
        );
        assert_debug_snapshot!(
            "AckElicitingTransmissionSet",
            size_of::<AckElicitingTransmissionSet>()
        );
    }
}
// COVERAGE_END_TEST
