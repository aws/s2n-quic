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
pub(crate) fn insert<T: IntervalBound + Ord>(
    ranges: &mut VecDeque<Interval<T>>,
    mut range: Interval<T>,
    start_index: usize,
    limit: Option<NonZeroUsize>,
) -> Result<usize, IntervalSetError> {
    // this range is intentionally invalid and will only be
    // valid if the `scan` method finds a match
    #[allow(clippy::reversed_empty_ranges)]
    let replace_range = usize::MAX..0;

    let mut insertion = Insertion { replace_range };

    let iter = ranges.iter().enumerate().skip(start_index);

    if let Some(index) = insertion.scan(iter, &mut range) {
        return Ok(index);
    }

    insertion.apply(ranges, range, limit)
}

/// A structure to keep temporary state for an insertion
#[derive(Debug)]
struct Insertion {
    replace_range: Range<usize>,
}

impl Insertion {
    /// Scans over the Intervals and updates the `Insertion` state with the
    /// applicable interval ranges
    ///
    /// Returns `Some(index)` if the `Interval` is already present in the iterator,
    /// otherwise `None` is returned.
    #[inline]
    fn scan<'a, T: 'a + Ord + IntervalBound, I: Iterator<Item = (usize, &'a Interval<T>)>>(
        &mut self,
        ranges: I,
        range_a: &mut Interval<T>,
    ) -> Option<usize> {
        use Ordering::*;

        for (slot_index, range_b) in ranges {
            match (
                range_a.start.cmp(&range_b.start),
                range_a.end.cmp(&range_b.end),
            ) {
                // the ranges are equal
                //
                // range A: |---------|
                // range B: |---------|
                //
                (Equal, Equal) |

                // range A is a subset of range B
                //
                // range A:     |-----|
                // range B: |---------|
                //
                // do nothing
                //
                (Greater, Equal) => return Some(slot_index + 1),

                // range A is a subset of range B
                //
                // range A: |-----|
                // range B: |---------|
                //
                // do nothing
                //
                (Equal, Less) |

                // range A is a subset of range B
                //
                // range A:    |----|
                // range B: |---------|
                //
                (Greater, Less) => return Some(slot_index),

                // range A is part of range B.
                //
                // Before:
                //
                // range A:      |--------|
                // range B: |--------|
                //
                // After:
                //
                // range A: |-------------|
                // range B: |
                //
                (Greater, Greater) if range_a.should_coalesce(range_b) => {
                    range_a.start = range_b.start;
                    self.set_start(slot_index);
                    let next_slot = slot_index + 1;
                    self.set_end(next_slot);
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
                // range A: |---------|
                // range B: |---|
                //
                // mark B as obsolete
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

                // range A ends with range B
                //
                // range A: |---------|
                // range B:       |---|
                //
                // mark B as obsolete and return
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
                // range A: |-------------|
                // range B:               |
                //
                (Less, Less) if range_b.should_coalesce(range_a) => {
                    range_a.end = range_b.end;
                    self.set_start(slot_index);
                    let next_slot = slot_index + 1;
                    self.set_end(next_slot);
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
                    self.set_start(slot_index);
                    self.set_end(slot_index);
                    break;
                }
            }
        }

        None
    }

    /// Applies the `Insertion` to the given set of `Interval`s.
    #[inline]
    fn apply<T>(
        self,
        ranges: &mut VecDeque<Interval<T>>,
        range: Interval<T>,
        limit: Option<NonZeroUsize>,
    ) -> Result<usize, IntervalSetError> {
        let replace_range = self.replace_range;
        let prev_len = ranges.len();

        let ensure_can_insert = || {
            let under_limit = if let Some(limit) = limit {
                limit.get() > prev_len
            } else {
                true
            };

            if under_limit {
                Ok(())
            } else {
                Err(IntervalSetError::LimitExceeded)
            }
        };

        let index = replace_range.start;
        let replace_count = if let Some(count) = replace_range.end.checked_sub(index) {
            count
        } else {
            ensure_can_insert()?;

            // add it to the end
            ranges.push_back(range);
            return Ok(prev_len);
        };

        match replace_count {
            0 => {
                ensure_can_insert()?;
                ranges.insert(index, range);
            }
            1 => {
                ranges[index] = range;
            }
            2 => {
                ranges[index] = range;
                ranges.remove(index + 1);
            }
            _ => {
                ranges[index] = range;
                ranges.drain(index + 1..replace_range.end);
            }
        };

        Ok(index)
    }

    #[inline]
    fn set_start(&mut self, start: usize) {
        self.replace_range.start = min(self.replace_range.start, start);
    }

    #[inline]
    fn set_end(&mut self, end: usize) {
        self.replace_range.end = max(self.replace_range.end, end);
    }
}
