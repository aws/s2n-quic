use crate::interval_set::{Interval, IntervalBound};
use alloc::collections::vec_deque::{self, VecDeque};
use core::cmp::Ordering;

/// An iterator of `Intervals` over the intersection of two sets,
/// i.e. the values in both `A` and `B` will be returned.
#[derive(Debug)]
pub struct Intersection<'a, T> {
    set_a: vec_deque::Iter<'a, Interval<T>>,
    set_b: vec_deque::Iter<'a, Interval<T>>,
    interval_a: Option<Interval<T>>,
    interval_b: Option<Interval<T>>,
}

impl<'a, T: Copy> Intersection<'a, T> {
    pub(crate) fn new(set_a: &'a VecDeque<Interval<T>>, set_b: &'a VecDeque<Interval<T>>) -> Self {
        let mut set_a = set_a.iter();
        let interval_a = set_a.next().cloned();

        let mut set_b = set_b.iter();
        let interval_b = set_b.next().cloned();

        Self {
            set_a,
            set_b,
            interval_a,
            interval_b,
        }
    }
}

impl<'a, T: 'a + IntervalBound + Ord> Iterator for Intersection<'a, T> {
    type Item = Interval<T>;

    fn next(&mut self) -> Option<Self::Item> {
        use Ordering::*;

        let interval_a = self.interval_a.as_mut()?;
        let interval_b = self.interval_b.as_mut()?;

        macro_rules! advance_set_a {
            () => {
                self.interval_a = self.set_a.next().cloned();
            };
        }

        macro_rules! advance_set_b {
            () => {
                self.interval_b = self.set_b.next().cloned();
            };
        }

        loop {
            match (
                interval_a.start.cmp(&interval_b.start),
                interval_a.end.cmp(&interval_b.end),
            ) {
                // interval A is a subset of interval B
                //
                // interval A: |-----|
                // interval B: |---------|
                //
                // Returns:
                //             |-----|
                //
                (Equal, Less) |

                // interval A is a subset of interval B
                //
                // interval A:    |----|
                // interval B: |---------|
                //
                // Returns:
                //                |----|
                //
                (Greater, Less) => {
                    let item = *interval_a;
                    interval_b.start = interval_a.end_exclusive();
                    advance_set_a!();
                    return Some(item);
                }

                // interval A contains interval B, spilling over into the next slot
                //
                // interval A: |---------|
                // interval B: |---|
                //
                // Returns:
                //             |---|
                //
                (Equal, Greater) |

                // interval A contains interval B, spilling over into the next slot
                //
                // interval A: |---------|
                // interval B:    |---|
                //
                // Returns:
                //                |---|
                //
                (Less, Greater) => {
                    let item = *interval_b;
                    interval_a.start = interval_b.end_exclusive();
                    advance_set_b!();
                    return Some(item);
                }

                // interval A is a subset of interval B
                //
                // interval A:     |-----|
                // interval B: |---------|
                //
                // Returns:
                //                 |-----|
                //
                (Greater, Equal) |

                // the intervals are equal
                //
                // interval A: |---------|
                // interval B: |---------|
                //
                // Returns:
                //             |---------|
                //
                (Equal, Equal) => {
                    let item = *interval_a;
                    advance_set_a!();
                    advance_set_b!();
                    return Some(item);
                }

                // interval A ends with interval B
                //
                // interval A: |---------|
                // interval B:       |---|
                //
                // Returns:
                //                   |---|
                //
                (Less, Equal) => {
                    let item = *interval_b;
                    advance_set_a!();
                    advance_set_b!();
                    return Some(item);
                }

                // interval A is part of interval B.
                //
                // interval A:      |--------|
                // interval B: |--------|
                //
                // Returns:
                //                  |---|
                //
                (Greater, Greater) if interval_a.start <= interval_b.end => {
                    let mut item = *interval_a;
                    item.end = interval_b.end;
                    interval_a.start = item.end_exclusive();
                    advance_set_b!();
                    return Some(item);
                }

                // interval A overlaps part of interval B
                //
                // interval A: |--------|
                // interval B:      |--------|
                //
                // Returns:
                //                  |---|
                //
                (Less, Less) if interval_a.end >= interval_b.start => {
                    let mut item = *interval_b;
                    item.end = interval_a.end;
                    interval_b.start = item.end_exclusive();
                    advance_set_a!();
                    return Some(item);
                }

                // interval A comes later
                //
                // interval A:          |-----|
                // interval B: |----|
                //
                // continue to next B
                //
                (Greater, Greater) => {
                    *interval_b = self.set_b.next().cloned()?;
                    continue;
                }

                // interval A comes before interval B
                //
                // interval A: |---|
                // interval B:        |--------|
                //
                // continue to next A
                //
                (Less, Less) => {
                    *interval_a = self.set_a.next().cloned()?;
                    continue;
                }
            }
        }
    }
}
