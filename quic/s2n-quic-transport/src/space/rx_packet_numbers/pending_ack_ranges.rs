// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{interval_set, space::rx_packet_numbers::ack_ranges::AckRanges};
use core::time::Duration;
use s2n_quic_core::{
    frame::ack::EcnCounts,
    packet::number::{PacketNumber, PacketNumberRange},
};

/// Stores ACK ranges pending processing
#[derive(Clone, Debug, Default)]
pub struct PendingAckRanges {
    ranges: AckRanges,
    ecn_counts: EcnCounts,
    ack_delay: Duration,
}

impl PendingAckRanges {
    #[inline]
    pub fn new(ranges: AckRanges, ecn_counts: EcnCounts, ack_delay: Duration) -> Self {
        PendingAckRanges {
            ranges,
            ecn_counts,
            ack_delay,
        }
    }

    /// Extend with a packet number range; dropping smaller values if needed
    #[inline]
    pub fn extend(
        &mut self,
        acked_packets: PacketNumberRange,
        ecn_counts: Option<EcnCounts>,
        ack_delay: Duration,
    ) -> bool {
        if let Some(ecn_counts) = ecn_counts {
            self.ecn_counts = self.ecn_counts.max(ecn_counts);
        }
        // TODO: at the moment only a single payload(single delivery) worth of ACKs is
        // batched for processing. This means that its acceptable to take the max
        // ack_delay value.
        //
        // Once multiple payloads are stored/batched, multiple ack_delays might need to
        // be stored.
        self.ack_delay = self.ack_delay.max(ack_delay);

        // TODO: add metrics if ack ranges are being dropped
        self.ranges.insert_packet_number_range(acked_packets)
    }

    /// Returns an iterator over all of the values contained in the ranges `IntervalSet`.
    #[inline]
    pub fn iter(&self) -> interval_set::Iter<PacketNumber> {
        self.ranges.iter()
    }

    /// Clear the ack ranges and reset values
    #[inline]
    pub fn clear(&mut self) {
        self.ranges.clear();
        self.ecn_counts = EcnCounts::default();
        self.ack_delay = Duration::default();
    }

    /// Returns if ack ranges are being tracked
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[cfg(test)]
pub mod tests {
    use super::{super::tests::packet_numbers_iter, *};
    use s2n_quic_core::{
        frame::ack::EcnCounts, inet::ExplicitCongestionNotification,
        packet::number::PacketNumberRange,
    };

    #[test]
    fn insert_pending_ack_range_test() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = PendingAckRanges::new(ack_ranges, ecn_counts, now);

        assert!(pending_ack_ranges.is_empty());

        // insert gaps up to the limit
        let (now, ecn_counts, pn_a, pn_range_a) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_a, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);
        assert!(!pending_ack_ranges.is_empty());
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 1);

        let (now, ecn_counts, pn_b, pn_range_b) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_b, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);

        let (now, ecn_counts, pn_c, pn_range_c) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_c, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);

        // ensure everything was inserted
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 3);
        assert!(pending_ack_ranges.ranges.contains(&pn_a));
        assert!(pending_ack_ranges.ranges.contains(&pn_b));
        assert!(pending_ack_ranges.ranges.contains(&pn_c));
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);

        // ensure large range is inserted and lower packets are removed
        let (now, ecn_counts, pn_d, pn_range_d) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_d, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);

        // ensure the previous smaller packet number was dropped
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 3);
        assert!(!pending_ack_ranges.ranges.contains(&pn_a));
        assert!(pending_ack_ranges.ranges.contains(&pn_b));
        assert!(pending_ack_ranges.ranges.contains(&pn_c));
        assert!(pending_ack_ranges.ranges.contains(&pn_d));

        // ensure smaller values are not recorded
        {
            assert!(!pending_ack_ranges.extend(pn_range_a, Some(ecn_counts), now));
            assert_eq!(pending_ack_ranges.ranges.interval_len(), 3);
            assert!(!pending_ack_ranges.ranges.contains(&pn_a));
            assert!(pending_ack_ranges.ranges.contains(&pn_b));
            assert!(pending_ack_ranges.ranges.contains(&pn_c));
            assert!(pending_ack_ranges.ranges.contains(&pn_d));
            assert_eq!(pending_ack_ranges.ack_delay, now);
            assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);
        }

        // ensure pending_ack_ranges clear functionality works
        {
            assert!(!pending_ack_ranges.is_empty());
            pending_ack_ranges.clear();

            assert!(pending_ack_ranges.is_empty());
            assert_eq!(pending_ack_ranges.ranges.interval_len(), 0);
            assert!(!pending_ack_ranges.ranges.contains(&pn_a));
            assert!(!pending_ack_ranges.ranges.contains(&pn_b));
            assert!(!pending_ack_ranges.ranges.contains(&pn_c));
            assert!(!pending_ack_ranges.ranges.contains(&pn_d));
        }
    }

    #[test]
    fn iterate_range_test() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = PendingAckRanges::new(ack_ranges, ecn_counts, now);

        // insert gaps up to the limit
        let (now, ecn_counts, pn_a, pn_range_a) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_a, Some(ecn_counts), now));

        let (now, ecn_counts, pn_b, pn_range_b) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_b, Some(ecn_counts), now));

        let (now, ecn_counts, pn_c, pn_range_c) =
            helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
        assert!(pending_ack_ranges.extend(pn_range_c, Some(ecn_counts), now));

        let coll: Vec<PacketNumber> = pending_ack_ranges.iter().collect();
        assert_eq!(coll.len(), 3);
        let arr = [pn_a, pn_b, pn_c];
        for pn in coll.iter() {
            assert!(arr.contains(pn));
        }

        // test eviction of pn range
        {
            let (now, ecn_counts, pn_d, pn_range_d) =
                helper_increment_and_new_pn_range(now, ecn_counts, &mut packet_numbers);
            assert!(pending_ack_ranges.extend(pn_range_d, Some(ecn_counts), now));

            let coll: Vec<PacketNumber> = pending_ack_ranges.iter().collect();
            assert_eq!(coll.len(), 3);
            assert!(!coll.contains(&pn_a));

            let arr = [pn_b, pn_c, pn_d];
            for pn in coll.iter() {
                assert!(arr.contains(pn));
            }
        }
    }

    #[test]
    fn overlapping_range_test() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = PendingAckRanges::new(ack_ranges, ecn_counts, now);

        // |---a---b---c---d---e---f---g---h---i---|
        //     ^0
        //         ^-1-^
        //                 ^--2----^
        //                             ^-3-^
        //                                 ^-4-^
        //             ^--1_2--^
        //     ^--0_1--^
        let pn_a = packet_numbers.next().unwrap();
        let pn_b = packet_numbers.next().unwrap();
        let pn_c = packet_numbers.next().unwrap();
        let pn_d = packet_numbers.next().unwrap();
        let pn_e = packet_numbers.next().unwrap();
        let pn_f = packet_numbers.next().unwrap();
        let pn_g = packet_numbers.next().unwrap();
        let pn_h = packet_numbers.next().unwrap();
        let pn_i = packet_numbers.next().unwrap();
        let range_0 = PacketNumberRange::new(pn_a, pn_a);
        let range_1 = PacketNumberRange::new(pn_b, pn_c);
        let range_2 = PacketNumberRange::new(pn_d, pn_f);
        let range_3 = PacketNumberRange::new(pn_g, pn_h);
        let range_4 = PacketNumberRange::new(pn_h, pn_i);
        let range_0_1 = PacketNumberRange::new(pn_a, pn_c);
        let range_1_2 = PacketNumberRange::new(pn_c, pn_e);

        assert!(pending_ack_ranges.extend(range_1, Some(ecn_counts), now));
        assert!(pending_ack_ranges.extend(range_2, Some(ecn_counts), now));
        assert!(pending_ack_ranges.extend(range_3, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 3);

        for pn in range_1 {
            assert!(pending_ack_ranges.ranges.contains(&pn));
        }
        for pn in range_2 {
            assert!(pending_ack_ranges.ranges.contains(&pn));
        }
        for pn in range_3 {
            assert!(pending_ack_ranges.ranges.contains(&pn));
        }

        // merge ranges 1 and 2
        assert!(pending_ack_ranges.extend(range_1_2, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 2);

        // insert range 0 at low end
        assert!(pending_ack_ranges.extend(range_0, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 3);

        // merge range 0_1 at low end
        assert!(pending_ack_ranges.extend(range_0_1, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 2);

        // merge new range at high end
        assert!(pending_ack_ranges.extend(range_4, Some(ecn_counts), now));
        assert_eq!(pending_ack_ranges.ranges.interval_len(), 2);
    }

    fn helper_increment_and_new_pn_range(
        mut now: Duration,
        mut ecn_counts: EcnCounts,
        packet_numbers: &mut std::iter::StepBy<impl Iterator<Item = PacketNumber>>,
    ) -> (Duration, EcnCounts, PacketNumber, PacketNumberRange) {
        now = now.saturating_add(Duration::from_millis(1));
        ecn_counts.increment(ExplicitCongestionNotification::Ect0);
        ecn_counts.increment(ExplicitCongestionNotification::Ect1);
        ecn_counts.increment(ExplicitCongestionNotification::Ce);

        let pn = packet_numbers.next().unwrap();
        let pn_range = PacketNumberRange::new(pn, pn);

        (now, ecn_counts, pn, pn_range)
    }
}
