// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::interval_set::{IntervalSet, RangeInclusiveIter};
use core::{
    convert::TryInto,
    num::NonZeroUsize,
    ops::{Bound, Deref, DerefMut, RangeInclusive},
};
use s2n_quic_core::{
    ack::Settings,
    frame::ack,
    packet::number::{PacketNumber, PacketNumberRange},
    varint::VarInt,
};

#[derive(Clone, Debug)]
pub struct AckRanges(IntervalSet<PacketNumber>);

impl Default for AckRanges {
    fn default() -> Self {
        Self::new(Settings::default().ack_ranges_limit as usize)
    }
}

impl AckRanges {
    pub fn new(limit: usize) -> Self {
        let limit = NonZeroUsize::new(limit).expect("limit should be nonzero");
        Self(IntervalSet::with_limit(limit))
    }

    /// Inserts a packet number; dropping smaller values if needed
    #[inline]
    pub fn insert_packet_number_range(
        &mut self,
        pn_range: PacketNumberRange,
    ) -> Result<(), AckRangesError> {
        let interval = (
            Bound::Included(pn_range.start()),
            Bound::Included(pn_range.end()),
        );
        if self.0.insert(interval).is_ok() {
            return Ok(());
        }

        // attempt to shed the lowest packet number ranges to make room for larger ones
        match self.0.pop_min() {
            Some(min) => {
                if min < pn_range.start() {
                    let insert_res = self.0.insert(interval);
                    debug_assert!(
                        insert_res.is_ok(),
                        "min range was removed, so it should be possible to insert another range",
                    );
                    insert_res.map_err(|_| AckRangesError::RangeInsertionFailed {
                        min: pn_range.start(),
                        max: pn_range.end(),
                    })?;

                    Err(AckRangesError::LowestRangeDropped {
                        min: min.start,
                        max: min.end,
                    })
                } else {
                    // new value is smaller than min so inset it back in the front
                    let _ = self.0.insert_front(min);
                    Err(AckRangesError::RangeInsertionFailed {
                        min: pn_range.start(),
                        max: pn_range.end(),
                    })
                }
            }
            None => {
                debug_assert!(
                    false,
                    "IntervalSet should have capacity and return lowest entry"
                );
                Err(AckRangesError::RangeInsertionFailed {
                    min: pn_range.start(),
                    max: pn_range.end(),
                })
            }
        }
    }

    /// Inserts a packet number; dropping smaller values if needed
    #[inline]
    pub fn insert_packet_number(
        &mut self,
        packet_number: PacketNumber,
    ) -> Result<(), AckRangesError> {
        self.insert_packet_number_range(PacketNumberRange::new(packet_number, packet_number))
    }

    /// Returns the overall range of the ack_ranges
    #[inline]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AckRangesError {
    RangeInsertionFailed {
        min: PacketNumber,
        max: PacketNumber,
    },
    LowestRangeDropped {
        min: PacketNumber,
        max: PacketNumber,
    },
}

#[cfg(test)]
mod tests {
    use bolero::check;
    use s2n_quic_core::{packet::number::PacketNumberSpace, varint};

    use super::{super::tests::packet_numbers_iter, *};

    #[test]
    fn insert_value_test() {
        let mut ack_ranges = AckRanges::new(3);
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number

        // insert gaps up to the limit
        let pn_a = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_a).is_ok());

        let pn_b = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_b).is_ok());

        let pn_c = packet_numbers.next().unwrap();
        assert!(ack_ranges.insert_packet_number(pn_c).is_ok());

        // ensure everything was inserted
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));

        // insert a new packet number gap
        let pn_d = packet_numbers.next().unwrap();
        assert_eq!(
            ack_ranges.insert_packet_number(pn_d).err().unwrap(),
            AckRangesError::LowestRangeDropped {
                min: pn_a,
                max: pn_a
            }
        );

        // ensure the previous smaller packet number was dropped
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(!ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));
        assert!(ack_ranges.contains(&pn_d));

        // ensure smaller values are not recorded
        assert_eq!(
            ack_ranges.insert_packet_number(pn_a).err().unwrap(),
            AckRangesError::RangeInsertionFailed {
                min: pn_a,
                max: pn_a
            }
        );
        assert_eq!(ack_ranges.interval_len(), 3);
        assert!(!ack_ranges.contains(&pn_a));
        assert!(ack_ranges.contains(&pn_b));
        assert!(ack_ranges.contains(&pn_c));
        assert!(ack_ranges.contains(&pn_d));
    }

    #[test]
    fn overlapping_range_test() {
        let mut packet_numbers = packet_numbers_iter().step_by(2); // skip every other packet number
        let mut ack_ranges = AckRanges::new(3);

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

        assert!(ack_ranges.insert_packet_number_range(range_1).is_ok());
        assert!(ack_ranges.insert_packet_number_range(range_2).is_ok());
        assert!(ack_ranges.insert_packet_number_range(range_3).is_ok());
        assert_eq!(ack_ranges.interval_len(), 3);

        for pn in range_1 {
            assert!(ack_ranges.contains(&pn));
        }
        for pn in range_2 {
            assert!(ack_ranges.contains(&pn));
        }
        for pn in range_3 {
            assert!(ack_ranges.contains(&pn));
        }

        // merge ranges 1 and 2
        assert!(ack_ranges.insert_packet_number_range(range_1_2).is_ok());
        assert_eq!(ack_ranges.interval_len(), 2);

        // insert range 0 at low end
        assert!(ack_ranges.insert_packet_number_range(range_0).is_ok());
        assert_eq!(ack_ranges.interval_len(), 3);

        // merge range 0_1 at low end
        assert!(ack_ranges.insert_packet_number_range(range_0_1).is_ok());
        assert_eq!(ack_ranges.interval_len(), 2);

        // merge new range at high end
        assert!(ack_ranges.insert_packet_number_range(range_4).is_ok());
        assert_eq!(ack_ranges.interval_len(), 2);
    }

    #[test]
    fn large_range_test() {
        let pn_a = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u32(1));
        let pn_b = PacketNumberSpace::ApplicationData
            .new_packet_number(VarInt::new(varint::MAX_VARINT_VALUE).unwrap());
        let mut ack_ranges = AckRanges::new(3);

        let range_1 = PacketNumberRange::new(pn_a, pn_b);

        assert!(ack_ranges.insert_packet_number_range(range_1).is_ok());
        assert_eq!(ack_ranges.interval_len(), 1);
    }

    #[test]
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("AckRanges", size_of::<AckRanges>());
    }

    #[test]
    fn insert_packet_number_range_fuzz() {
        check!()
            .with_type::<(u32, u32)>()
            .map(|(a, b)| (a.min(b), a.max(b))) // ensure valid range
            .for_each(|(a, b)| {
                let mut ack_ranges = AckRanges::new(1);

                let pn_a = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u32(*a));
                let pn_b = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u32(*b));
                let range_1 = PacketNumberRange::new(pn_a, pn_b);

                assert!(ack_ranges.insert_packet_number_range(range_1).is_ok());
            });
    }
}
