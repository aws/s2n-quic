// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::IntervalSetError;
use core::{
    cmp::Ordering,
    fmt,
    ops::{Bound, Range, RangeBounds, RangeInclusive},
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Interval<T> {
    pub(super) start: T,
    pub(super) end: T,
}

impl<T: IntervalBound> Interval<T> {
    pub fn from_range_bounds<B: RangeBounds<T>>(bounds: B) -> Result<Self, IntervalSetError> {
        let start = match bounds.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => start.step_down().ok_or(IntervalSetError::InvalidInterval)?,
            _ => return Err(IntervalSetError::InvalidInterval),
        };

        let end = match bounds.end_bound() {
            Bound::Included(end) => *end,
            Bound::Excluded(end) => end.step_down().ok_or(IntervalSetError::InvalidInterval)?,
            _ => return Err(IntervalSetError::InvalidInterval),
        };

        let interval = Self { start, end };

        if interval.is_valid() {
            Ok(interval)
        } else {
            Err(IntervalSetError::InvalidInterval)
        }
    }

    #[inline]
    pub fn start_inclusive(&self) -> T {
        self.start
    }

    #[inline]
    pub fn start_exclusive(&self) -> T {
        self.start.step_down_saturating()
    }

    #[inline]
    pub fn end_inclusive(&self) -> T {
        self.end
    }

    #[inline]
    pub fn end_exclusive(&self) -> T {
        self.end.step_up_saturating()
    }

    #[inline]
    pub fn len(&self) -> usize {
        // Interval always has at least 1
        let interval_base_len = 1;
        interval_base_len + self.start.steps_between(&self.end)
    }

    #[inline]
    pub fn set_len(&mut self, len: usize) {
        debug_assert_ne!(len, 0, "Intervals cannot be 0 length");
        self.end = self.start.steps_between_len(len);
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        // Interval always has at least 1
        false
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.end >= self.start
    }

    #[inline]
    pub(crate) fn should_coalesce(&self, other: &Self) -> bool {
        self.start <= other.end_exclusive()
    }
}

impl<T: IntervalBound> IntoIterator for &Interval<T> {
    type IntoIter = Interval<T>;
    type Item = T;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        *self
    }
}

impl<T> RangeBounds<T> for Interval<T> {
    #[inline]
    fn start_bound(&self) -> Bound<&T> {
        Bound::Included(&self.start)
    }

    #[inline]
    fn end_bound(&self) -> Bound<&T> {
        Bound::Included(&self.end)
    }
}

impl<T> RangeBounds<T> for &Interval<T> {
    #[inline]
    fn start_bound(&self) -> Bound<&T> {
        Bound::Included(&self.start)
    }

    #[inline]
    fn end_bound(&self) -> Bound<&T> {
        Bound::Included(&self.end)
    }
}

impl<T: Ord> PartialEq<T> for Interval<T> {
    #[inline]
    fn eq(&self, value: &T) -> bool {
        self.partial_cmp(value) == Some(Ordering::Equal)
    }
}

impl<T: Ord> PartialOrd<T> for Interval<T> {
    #[inline]
    fn partial_cmp(&self, value: &T) -> Option<Ordering> {
        use Ordering::*;
        Some(match (self.start.cmp(value), self.end.cmp(value)) {
            (Equal, _) => Equal,
            (_, Equal) => Equal,
            (Greater, Less) => Equal,
            (Less, Greater) => Equal,
            (Less, Less) => Less,
            (Greater, Greater) => Greater,
        })
    }
}

impl<T: IntervalBound> From<&Interval<T>> for Interval<T> {
    #[inline]
    fn from(interval: &Interval<T>) -> Self {
        *interval
    }
}

macro_rules! range_impls {
    ($range_ty:ident, $into:expr, $from:expr) => {
        impl<T: IntervalBound> From<Interval<T>> for $range_ty<T> {
            #[inline]
            fn from(range: Interval<T>) -> Self {
                #[allow(clippy::redundant_closure_call)]
                ($into)(range)
            }
        }

        impl<T: IntervalBound> From<&Interval<T>> for $range_ty<T> {
            #[inline]
            fn from(range: &Interval<T>) -> Self {
                #[allow(clippy::redundant_closure_call)]
                ($into)(*range)
            }
        }

        impl<T: IntervalBound> From<$range_ty<T>> for Interval<T> {
            #[inline]
            fn from(range: $range_ty<T>) -> Self {
                #[allow(clippy::redundant_closure_call)]
                ($from)(range)
            }
        }

        impl<'a, T: IntervalBound> From<&'a $range_ty<T>> for Interval<T> {
            #[inline]
            fn from(range: &'a $range_ty<T>) -> Self {
                #[allow(clippy::redundant_closure_call)]
                ($from)(range.clone())
            }
        }

        impl<T: IntervalBound> PartialEq<$range_ty<T>> for Interval<T> {
            #[inline]
            fn eq(&self, other: &$range_ty<T>) -> bool {
                self.partial_cmp(other) == Some(Ordering::Equal)
            }
        }

        impl<T: IntervalBound> PartialEq<$range_ty<T>> for &Interval<T> {
            #[inline]
            fn eq(&self, other: &$range_ty<T>) -> bool {
                self.partial_cmp(other) == Some(Ordering::Equal)
            }
        }

        impl<T: IntervalBound> PartialOrd<$range_ty<T>> for Interval<T> {
            #[inline]
            fn partial_cmp(&self, other: &$range_ty<T>) -> Option<Ordering> {
                let other: Self = other.into();
                self.partial_cmp(&other)
            }
        }

        impl<T: IntervalBound> PartialOrd<$range_ty<T>> for &Interval<T> {
            #[inline]
            fn partial_cmp(&self, other: &$range_ty<T>) -> Option<Ordering> {
                let other: Interval<_> = other.into();
                (*self).partial_cmp(&other)
            }
        }
    };
}

range_impls!(
    Range,
    |interval: Interval<_>| interval.start..interval.end_exclusive(),
    |range: Range<_>| Self {
        start: range.start,
        end: range.end.step_down_saturating(),
    }
);

range_impls!(
    RangeInclusive,
    |interval: Interval<_>| interval.start..=interval.end,
    |range: RangeInclusive<_>| {
        let (start, end) = range.into_inner();
        Self { start, end }
    }
);

impl<T: IntervalBound> Iterator for Interval<T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        let current = self.start;
        if current > self.end {
            return None;
        }
        if let Some(next) = current.step_up() {
            self.start = next;
        } else {
            self.end = current.step_down()?;
        }
        Some(current)
    }
}

impl<T: IntervalBound> DoubleEndedIterator for Interval<T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        let current = self.end;
        if current < self.start {
            return None;
        }
        if let Some(next) = current.step_down() {
            self.end = next;
        } else {
            self.start = current.step_up()?;
        }
        Some(current)
    }
}

impl<T: fmt::Debug> fmt::Debug for Interval<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        ((&self.start)..=(&self.end)).fmt(f)
    }
}

pub trait IntervalBound: Copy + Ord + Sized {
    fn step_up(self) -> Option<Self>;
    fn step_down(self) -> Option<Self>;
    fn steps_between(&self, upper: &Self) -> usize;
    fn steps_between_len(&self, len: usize) -> Self;

    fn step_up_saturating(self) -> Self {
        self.step_up().unwrap_or(self)
    }

    fn step_down_saturating(self) -> Self {
        self.step_down().unwrap_or(self)
    }
}

macro_rules! integer_bounds {
    ($type:ident) => {
        impl IntervalBound for $type {
            #[inline]
            fn step_up(self) -> Option<Self> {
                self.checked_add(1)
            }

            #[inline]
            fn step_down(self) -> Option<Self> {
                self.checked_sub(1)
            }

            #[inline]
            fn steps_between(&self, upper: &Self) -> usize {
                use core::convert::TryInto;
                (upper - self).try_into().unwrap_or(usize::MAX)
            }

            #[inline]
            fn steps_between_len(&self, len: usize) -> Self {
                use core::convert::TryInto;
                let len: Self = (len - 1).try_into().unwrap();
                *self + len
            }
        }
    };
}

integer_bounds!(u8);
integer_bounds!(i8);
integer_bounds!(u16);
integer_bounds!(i16);
integer_bounds!(u32);
integer_bounds!(i32);
integer_bounds!(u64);
integer_bounds!(i64);
integer_bounds!(u128);
integer_bounds!(i128);
integer_bounds!(usize);
integer_bounds!(isize);

use crate::{packet::number::PacketNumber, varint::VarInt};

impl IntervalBound for VarInt {
    #[inline]
    fn step_up(self) -> Option<Self> {
        self.checked_add(Self::from_u8(1))
    }

    #[inline]
    fn step_down(self) -> Option<Self> {
        self.checked_sub(Self::from_u8(1))
    }

    #[inline]
    fn steps_between(&self, upper: &Self) -> usize {
        <u64 as IntervalBound>::steps_between(self, upper)
    }

    #[inline]
    fn steps_between_len(&self, len: usize) -> Self {
        *self + (len - 1)
    }
}

impl IntervalBound for PacketNumber {
    #[inline]
    fn step_up(self) -> Option<Self> {
        self.next()
    }

    #[inline]
    fn step_down(self) -> Option<Self> {
        let space = self.space();
        let value = PacketNumber::as_varint(self);
        Some(space.new_packet_number(value.step_down()?))
    }

    #[inline]
    fn steps_between(&self, upper: &Self) -> usize {
        let lower = PacketNumber::as_varint(*self);
        let upper = PacketNumber::as_varint(*upper);
        lower.steps_between(&upper)
    }

    #[inline]
    fn steps_between_len(&self, len: usize) -> Self {
        let start = PacketNumber::as_varint(*self);
        let end = start.steps_between_len(len);
        self.space().new_packet_number(end)
    }
}
