use crate::interval_set::{IntervalSet, RangeInclusiveIter};
use core::{
    convert::TryInto,
    num::NonZeroUsize,
    ops::{Deref, DerefMut, RangeInclusive},
};
use s2n_quic_core::{frame::ack, packet::number::PacketNumber, varint::VarInt};

#[derive(Clone, Debug)]
pub struct AckRanges(IntervalSet<PacketNumber>);

impl Default for AckRanges {
    fn default() -> Self {
        Self::new(DEFAULT_ACK_RANGES_LIMIT)
    }
}

pub const DEFAULT_ACK_RANGES_LIMIT: usize = 10;

impl AckRanges {
    pub fn new(limit: usize) -> Self {
        let limit = NonZeroUsize::new(limit).expect("limit should be nonzero");
        Self(IntervalSet::with_limit(limit))
    }

    /// Inserts a packet number; dropping smaller values if needed
    pub fn insert_packet_number(&mut self, packet_number: PacketNumber) -> bool {
        if self.0.insert_value(packet_number).is_ok() {
            return true;
        } else {
            // shed the lowest packet number ranges to make room for larger ones
            if let Some(min) = self.0.pop_min() {
                return if min < packet_number {
                    self.0.insert_value(packet_number).is_ok()
                } else {
                    let _ = self.0.insert_front(min);
                    false
                };
            }
        }

        false
    }

    /// Returns the overall range of the ack_ranges
    pub fn spread(&self) -> usize {
        match (self.min_value(), self.max_value()) {
            (Some(min), Some(max)) => {
                let min = PacketNumber::as_varint(min);
                let max = PacketNumber::as_varint(max);
                (max - min).try_into().unwrap_or(core::usize::MAX)
            }
            _ => 0,
        }
    }
}

type AckRangesIter<'a> = core::iter::Map<
    core::iter::Rev<RangeInclusiveIter<'a, PacketNumber>>,
    fn(RangeInclusive<PacketNumber>) -> RangeInclusive<VarInt>,
>;

impl<'a> ack::AckRanges for &'a AckRanges {
    type Iter = AckRangesIter<'a>;

    fn ack_ranges(&self) -> Self::Iter {
        self.0.inclusive_ranges().rev().map(|range| {
            let (start, end) = range.into_inner();
            PacketNumber::as_varint(start)..=PacketNumber::as_varint(end)
        })
    }
}

impl Deref for AckRanges {
    type Target = IntervalSet<PacketNumber>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AckRanges {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
pub mod tests {
    use super::{super::tests::packet_numbers_iter, *};

    #[test]
    fn insert_value_test() {
        let mut ack_ranges = AckRanges::new(3);
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number

        // insert gaps up to the limit
        let pn_a = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_a));

        let pn_b = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_b));

        let pn_c = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_c));

        // ensure everything was inserted
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));

        // insert a new packet number gap
        let pn_d = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_d));

        // ensure the previous smaller packet number was dropped
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(!ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));
        assert!(ack_ranges.contains(&pn_d));

        // ensure smaller values are not recorded
        assert!(!ack_ranges.insert_packet_number(pn_a));
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(!ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));
        assert!(ack_ranges.contains(&pn_d));
    }

    #[test]
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("AckRanges", size_of::<AckRanges>());
    }
}
