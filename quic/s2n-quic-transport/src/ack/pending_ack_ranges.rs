// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{ack::ack_ranges::AckRanges, path};
use core::time::Duration;
use s2n_quic_core::{
    frame::ack::EcnCounts,
    packet::number::{PacketNumber, PacketNumberRange},
};

/// Stores aggregated ACK info for delayed processing
#[derive(Clone, Debug, Default)]
pub struct PendingAckRanges {
    ack_ranges: AckRanges,
    ecn_counts: EcnCounts,
    ack_delay: Duration,
    /// The path for which to aggregate ACKs
    pub current_active_path: Option<path::Id>,
}

impl PendingAckRanges {
    /// Extend with a packet number range; dropping smaller values if needed
    #[inline]
    pub fn extend(
        &mut self,
        acked_packets: impl Iterator<Item = PacketNumberRange>,
        ecn_counts: Option<EcnCounts>,
        ack_delay: Duration,
    ) -> Result<(), ()> {
        debug_assert!(
            self.current_active_path.is_some(),
            "active path should be set prior to inserting acks"
        );

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

        let mut did_insert = true;
        for range in acked_packets {
            did_insert &= self.ack_ranges.insert_packet_number_range(range).is_ok()
        }

        match did_insert {
            true => Ok(()),
            false => Err(()),
        }
    }

    /// Returns an iterator over all values in the `AckRanges`
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = PacketNumberRange> + '_ {
        debug_assert!(
            self.current_active_path.is_some(),
            "active path should be set prior to processing acks"
        );

        self.ack_ranges
            .inclusive_ranges()
            .into_iter()
            .map(|ack_range| PacketNumberRange::new(*ack_range.start(), *ack_range.end()))
    }

    /// Returns `EcnCounts` aggregated over all the pending ACKs
    #[inline]
    pub fn ecn_counts(&self) -> Option<EcnCounts> {
        debug_assert!(
            self.current_active_path.is_some(),
            "active path should be set prior to processing acks"
        );

        if self.ack_ranges.is_empty() {
            None
        } else {
            Some(self.ecn_counts)
        }
    }

    /// Returns the ACK delay associated with all the pending ACKs
    #[inline]
    pub fn ack_delay(&self) -> Duration {
        debug_assert!(
            self.current_active_path.is_some(),
            "active path should be set prior to processing acks"
        );

        self.ack_delay
    }

    /// Set the current active path for which to aggregate ACKs
    #[inline]
    pub fn set_active_path(&mut self, path_id: path::Id) {
        self.current_active_path = Some(path_id)
    }

    /// Returns the largest `PacketNumber` stored in the AckRanges.
    ///
    /// If no items are present in the set, `None` is returned.
    pub fn max_value(&self) -> Option<PacketNumber> {
        debug_assert!(
            self.current_active_path.is_some(),
            "active path should be set prior to processing acks"
        );

        self.ack_ranges.max_value()
    }

    /// Clear the aggregated ACK information
    #[inline]
    pub fn reset_aggregate_info(&mut self) {
        self.ack_ranges.clear();
        self.ecn_counts = EcnCounts::default();
        self.ack_delay = Duration::default();
    }

    /// Re-initialize all fields.
    ///
    /// Should be called at the end of a processing round to clear all
    /// data. Resets aggregated ack information and the current_active_path.
    #[inline]
    pub fn reset(&mut self) {
        debug_assert!(
            self.current_active_path.is_some(),
            "reset called more than once"
        );
        self.reset_aggregate_info();
        self.current_active_path = None;
    }

    /// Returns if ack ranges are being tracked
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ack_ranges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{super::tests::packet_numbers_iter, *};
    use bolero::check;
    use s2n_quic_core::{
        frame::ack::EcnCounts,
        inet::ExplicitCongestionNotification,
        packet::number::{PacketNumberRange, PacketNumberSpace},
        path::Id,
        varint::{self, VarInt},
    };

    pub fn helper_new_pending(
        ack_ranges: AckRanges,
        ecn_counts: EcnCounts,
        ack_delay: Duration,
    ) -> PendingAckRanges {
        PendingAckRanges {
            ack_ranges,
            ecn_counts,
            ack_delay,
            current_active_path: None,
        }
    }

    #[test]
    fn pending_ack_ranges_test() {
        let mut now = Duration::from_millis(0);
        let mut ecn_counts = EcnCounts::default();
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = helper_new_pending(ack_ranges, ecn_counts, now);
        pending_ack_ranges.set_active_path(Id::test_id());

        assert!(pending_ack_ranges.is_empty());

        // insert range with ack_delay and ecn_counts
        now = now.saturating_add(Duration::from_millis(1));
        ecn_counts.increment(ExplicitCongestionNotification::Ect0);
        ecn_counts.increment(ExplicitCongestionNotification::Ect1);
        ecn_counts.increment(ExplicitCongestionNotification::Ce);
        let pn_a = packet_numbers.next().unwrap();
        let pn_range_a = Some(PacketNumberRange::new(pn_a, pn_a));

        assert!(pending_ack_ranges
            .extend(pn_range_a.into_iter(), Some(ecn_counts), now)
            .is_ok());

        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);
        assert!(!pending_ack_ranges.is_empty());
        assert_eq!(pending_ack_ranges.ack_ranges.interval_len(), 1);

        // insert new range with updated ack_delay and ecn_counts
        now = now.saturating_add(Duration::from_millis(1));
        ecn_counts.increment(ExplicitCongestionNotification::Ect0);
        ecn_counts.increment(ExplicitCongestionNotification::Ect1);
        ecn_counts.increment(ExplicitCongestionNotification::Ce);
        let pn_b = packet_numbers.next().unwrap();
        let pn_range_b = Some(PacketNumberRange::new(pn_b, pn_b));

        assert!(pending_ack_ranges
            .extend(pn_range_b.into_iter(), Some(ecn_counts), now)
            .is_ok());

        assert_eq!(pending_ack_ranges.ack_delay, now);
        assert_eq!(pending_ack_ranges.ecn_counts, ecn_counts);
        assert!(!pending_ack_ranges.is_empty());
        assert_eq!(pending_ack_ranges.ack_ranges.interval_len(), 2);

        // ensure pending_ack_ranges clear functionality works
        {
            assert!(!pending_ack_ranges.is_empty());
            pending_ack_ranges.reset_aggregate_info();

            assert!(pending_ack_ranges.is_empty());
            assert_eq!(pending_ack_ranges.ack_ranges.interval_len(), 0);
            assert!(!pending_ack_ranges.ack_ranges.contains(&pn_a));
            assert!(!pending_ack_ranges.ack_ranges.contains(&pn_b));
        }
    }

    #[test]
    fn iterate_range_test() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = helper_new_pending(ack_ranges, ecn_counts, now);
        pending_ack_ranges.set_active_path(Id::test_id());

        // insert ranges
        let pn_a = packet_numbers.next().unwrap();
        let pn_range_a = Some(PacketNumberRange::new(pn_a, pn_a));
        assert!(pending_ack_ranges
            .extend(pn_range_a.into_iter(), Some(ecn_counts), now)
            .is_ok());

        let pn_b = packet_numbers.next().unwrap();
        let pn_range_b = Some(PacketNumberRange::new(pn_b, pn_b));
        assert!(pending_ack_ranges
            .extend(pn_range_b.into_iter(), Some(ecn_counts), now)
            .is_ok());

        let coll: Vec<PacketNumber> = pending_ack_ranges.iter().flatten().collect();
        assert_eq!(coll.len(), 2);
        let arr = [pn_a, pn_b];
        for pn in coll.iter() {
            assert!(arr.contains(pn));
        }
    }

    #[test]
    fn large_range_test() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        let pn_a = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u32(1));
        let pn_b = PacketNumberSpace::ApplicationData
            .new_packet_number(VarInt::new(varint::MAX_VARINT_VALUE).unwrap());
        let ack_ranges = AckRanges::new(3);
        let mut pending_ack_ranges = helper_new_pending(ack_ranges, ecn_counts, now);
        pending_ack_ranges.set_active_path(Id::test_id());

        let range_1 = Some(PacketNumberRange::new(pn_a, pn_b));

        assert!(pending_ack_ranges
            .extend(range_1.into_iter(), Some(ecn_counts), now)
            .is_ok());
        assert_eq!(pending_ack_ranges.ack_ranges.interval_len(), 1);
    }

    #[test]
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("PendingAckRanges", size_of::<PendingAckRanges>());
    }

    #[test]
    fn extend_fuzz() {
        let now = Duration::from_millis(0);
        let ecn_counts = EcnCounts::default();
        check!()
            .with_type::<(u32, u32)>()
            .map(|(a, b)| (a.min(b), a.max(b))) // ensure valid range
            .for_each(|(a, b)| {
                let ack_ranges = AckRanges::new(1);
                let mut pending_ack_ranges = helper_new_pending(ack_ranges, ecn_counts, now);
                pending_ack_ranges.set_active_path(Id::test_id());

                let pn_a = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u32(*a));
                let pn_b = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u32(*b));

                let range_1 = Some(PacketNumberRange::new(pn_a, pn_b));

                assert!(pending_ack_ranges
                    .extend(range_1.into_iter(), Some(ecn_counts), now)
                    .is_ok());
            });
    }
}
