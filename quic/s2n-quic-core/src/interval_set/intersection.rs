// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

/// Apply the intersection of `set_a` with `set_b` to `set_a`
pub(super) fn apply<T: IntervalBound>(
    set_a: &mut VecDeque<Interval<T>>,
    set_b: &VecDeque<Interval<T>>,
) {
    use Ordering::*;

    if set_a.is_empty() {
        return;
    }

    if set_b.is_empty() {
        set_a.clear();
        return;
    }

    let mut set_b = set_b.iter();
    let mut interval_b = set_b.next().expect("set_b is not empty");
    let mut a_index = 0;

    macro_rules! advance_set_a {
        () => {
            a_index += 1;
        };
    }

    macro_rules! advance_set_b {
        () => {
            if let Some(next_interval_b) = set_b.next() {
                interval_b = next_interval_b;
            } else {
                // the remainder of set_a is not in the intersection, since we've
                // reached the end of set_b
                set_a.truncate(a_index);
                return;
            }
        };
    }

    // Inserts a new interval into the next index of set_a that starts where the current interval b
    // ends (with a gap of 1) and ends where the current interval a ends
    macro_rules! split_off_a {
        ($interval_a:ident) => {
            // step_up_saturating is used because the next intersecting interval should not be
            // contiguous with the current interval, so we can expect a gap of at least 1.
            let new_interval_a: Interval<T> =
                (interval_b.end_exclusive().step_up_saturating()..=$interval_a.end).into();

            if new_interval_a.is_valid() {
                // if interval_a had only overlapped interval_b by 1, then the new interval
                // will not be valid and we can skip inserting it
                // for example: interval_a = [0..=48], interval_b = [0..=47]
                // new_interval_a = [49..=48] since we know 48 is not in interval_b
                set_a.insert(a_index + 1, new_interval_a);
            }
        };
    }

    while let Some(interval_a) = set_a.get(a_index) {
        match (interval_a.start.cmp(&interval_b.start),
               interval_a.end.cmp(&interval_b.end),
        ) {
            // interval A is a subset of interval B
            //
            // interval A: |-----|
            // interval B: |---------|
            //
            // End state:
            //             |-----|
            // a_index:           ^
            (Equal, Less) |

            // interval A is a subset of interval B
            //
            // interval A:    |----|
            // interval B: |---------|
            //
            // End state:
            //                |----|
            // a_index:             ^
            (Greater, Less) => {
                advance_set_a!();
            }

            // interval A contains interval B, spilling over into the next slot
            //
            // interval A: |---------|
            // interval B: |---|
            //
            // End state:
            //             |---| |---|
            // a_index:          ^
            (Equal, Greater) |

            // interval A contains interval B, spilling over into the next slot
            //
            // interval A: |---------|
            // interval B:   |---|
            //
            // End state:
            //               |---| |-|
            // a_index:            ^
            (Less, Greater) => {
                // split off the part of interval A that is spilling into the next slot
                split_off_a!(interval_a);
                set_a[a_index] = *interval_b;
                advance_set_a!();
                advance_set_b!();
            }

            // interval A is a subset of interval B
            //
            // interval A:     |-----|
            // interval B: |---------|
            //
            // End state:
            //                 |-----|
            // a_index:               ^
            (Greater, Equal) |

            // the intervals are equal
            //
            // interval A: |---------|
            // interval B: |---------|
            //
            // End state:
            //             |---------|
            // a_index:               ^
            (Equal, Equal) => {
                advance_set_a!();
                advance_set_b!();
                continue;
            }

            // interval A ends with interval B
            //
            // interval A: |---------|
            // interval B:       |---|
            //
            // End state:
            //                   |---|
            // a_index:               ^
            (Less, Equal) => {
                set_a[a_index].start = interval_b.start;
                advance_set_a!();
                advance_set_b!();
            }

            // interval A overlaps part of interval B.
            //
            // interval A:      |--------|
            // interval B: |--------|
            //
            // End state:
            //                  |---| |--|
            // a_index:               ^
            (Greater, Greater) if interval_a.start <= interval_b.end => {
                // split off the part of interval A that is spilling into the next slot
                split_off_a!(interval_a);
                set_a[a_index].end = interval_b.end;
                advance_set_a!();
                advance_set_b!();
            }

            // interval A overlaps part of interval B
            //
            // interval A: |--------|
            // interval B:      |--------|
            //
            // End state:
            //                  |---|
            // a_index:              ^
            (Less, Less) if interval_a.end >= interval_b.start => {
                set_a[a_index].start = interval_b.start;
                advance_set_a!();
            }

            // interval A comes later
            //
            // interval A:          |-----|
            // interval B: |----|
            //
            // continue to next B
            //
            (Greater, Greater) => {
                advance_set_b!();
            }

            // interval A comes before interval B
            //
            // interval A: |---|
            // interval B:        |--------|
            //
            // remove A and continue to next A
            //
            (Less, Less) => {
                set_a.remove(a_index);
            }
        }
    }
}
