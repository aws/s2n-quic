// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::interval_set::{Interval, IntervalBound, IntervalSetError};
use alloc::collections::VecDeque;
use core::{
    cmp::{max, min, Ordering},
    num::NonZeroUsize,
    ops::Range,
};

#[inline]
pub(crate) fn remove<T: IntervalBound + Ord>(
    ranges: &mut VecDeque<Interval<T>>,
    range: Interval<T>,
    start_index: usize,
    limit: Option<NonZeroUsize>,
) -> Result<usize, IntervalSetError> {
    // this range is intentionally invalid and will only be
    // valid if the `scan` method finds a match
    #[allow(clippy::reversed_empty_ranges)]
    let replace_range = usize::MAX..0;

    let can_push_range = limit.map(|l| l.get() > ranges.len() + 1).unwrap_or(true);

    let mut removal = Removal {
        replace_range,
        push_range: None,
        can_push_range,
    };

    let iter = ranges.iter_mut().enumerate().skip(start_index);

    if let Some(index) = removal.scan(iter, &range) {
        return Ok(index);
    }

    removal.apply(ranges)
}

/// A structure to keep temporary state for a removal
#[derive(Debug)]
struct Removal<T> {
    replace_range: Range<usize>,
    push_range: Option<Interval<T>>,
    can_push_range: bool,
}

impl<'a, T: 'a + IntervalBound + Ord> Removal<T> {
    /// Scans over the Intervals and updates the `Removal` state with the
    /// applicable interval ranges
    ///
    /// Returns `Some(index)` if the removal can be applied to a single interval
    #[inline]
    fn scan<I: Iterator<Item = (usize, &'a mut Interval<T>)>>(
        &mut self,
        ranges: I,
        range_a: &Interval<T>,
    ) -> Option<usize> {
        use Ordering::*;

        for (slot_index, range_b) in ranges {
            match (
                range_a.start.cmp(&range_b.start),
                range_a.end.cmp(&range_b.end),
            ) {
                // range A is a subset of range B
                //
                // Before:
                //
                // range A: |-----|
                // range B: |---------|
                //
                // After:
                //
                // range A: |-----|
                // range B:       |---|
                //
                (Equal, Less) => {
                    range_b.start = range_a.end_exclusive();
                    return Some(slot_index);
                }

                // range A is a subset of range B
                //
                // Before:
                //
                // range A:     |-----|
                // range B: |---------|
                //
                // After:
                //
                // range A:     |-----|
                // range B: |---|
                //
                (Greater, Equal) => {
                    range_b.end = range_a.start_exclusive();
                    return Some(slot_index + 1);
                }

                // range A is a subset of range B
                //
                // Before:
                //
                // range A:    |----|
                // range B: |---------|
                //
                // After:
                //
                // range A:    |----|
                // range B: |--|    |-|
                //
                (Greater, Less) => {
                    self.push_range(range_a.end_exclusive(), range_b.end)?;
                    range_b.end = range_a.start_exclusive();
                    let next_slot = slot_index + 1;
                    self.set_start(next_slot);
                    self.set_end(next_slot);
                    break;
                }

                // range A is part of range B.
                //
                // Before:
                //
                // range A:      |--------|
                // range B: |--------|
                //
                // After:
                //
                // range A:      |--------|
                // range B: |----|
                //
                (Greater, Greater) if range_a.should_coalesce(range_b) => {
                    range_b.end = range_a.start_exclusive();
                    let next_slot = slot_index + 1;
                    self.set_start(next_slot);
                    continue;
                }

                // range A comes later
                //
                // range A:          |-----|
                // range B: |----|
                //
                (Greater, Greater) => {
                    continue;
                }

                // range A contains range B, spilling over into the next slot
                //
                // Before:
                //
                // range A: |---------|
                // range B: |---|
                //
                // After:
                //
                // range A: |---------|
                // range B: |
                //
                (Equal, Greater) |

                // range A contains range B, spilling over into the next slot
                //
                // range A: |---------|
                // range B:    |---|
                //
                // mark B as obsolete
                //
                (Less, Greater) => {
                    self.set_start(slot_index);
                    let next_slot = slot_index + 1;
                    self.set_end(next_slot);
                    continue;
                }

                // the ranges are equal
                //
                // range A: |---------|
                // range B: |---------|
                //
                (Equal, Equal) |

                // range A ends with range B
                //
                // range A: |---------|
                // range B:       |---|
                //
                // mark B as obsolete
                //
                (Less, Equal) => {
                    self.set_start(slot_index);
                    let next_slot = slot_index + 1;
                    self.set_end(next_slot);
                    break;
                }

                // range A overlaps part of range B
                //
                // Before:
                //
                // range A: |--------|
                // range B:      |--------|
                //
                // After:
                //
                // range A: |--------|
                // range B:          |----|
                //
                (Less, Less) if range_b.should_coalesce(range_a)  => {
                    range_b.start = range_a.end_exclusive();
                    self.set_end(slot_index);
                    break;
                }

                // range A comes before range B
                //
                // range A: |---|
                // range B:        |--------|
                //
                // returns; no more searching needed
                //
                (Less, Less) => {
                    break;
                }
            }
        }

        None
    }

    /// Applies the `Removal` to the given set of `Interval`s.
    #[inline]
    fn apply(self, ranges: &mut VecDeque<Interval<T>>) -> Result<usize, IntervalSetError> {
        let replace_range = self.replace_range;

        let index = replace_range.start;

        if let Some(interval) = self.push_range {
            if self.can_push_range {
                ranges.insert(index, interval);
                return Ok(index);
            } else {
                return Err(IntervalSetError::LimitExceeded);
            }
        }

        match replace_range.end.checked_sub(index) {
            None => Ok(0),
            Some(0) => Ok(index),
            Some(1) => {
                ranges.remove(index);
                Ok(index)
            }
            Some(_) => {
                ranges.drain(replace_range);
                Ok(index)
            }
        }
    }

    #[inline]
    fn set_start(&mut self, start: usize) {
        self.replace_range.start = min(self.replace_range.start, start);
    }

    #[inline]
    fn set_end(&mut self, end: usize) {
        self.replace_range.end = max(self.replace_range.end, end);
    }

    #[inline]
    fn push_range(&mut self, start: T, end: T) -> Option<()> {
        debug_assert!(self.push_range.is_none());
        self.push_range = Some(Interval { start, end });

        if self.can_push_range {
            Some(())
        } else {
            None
        }
    }
}
