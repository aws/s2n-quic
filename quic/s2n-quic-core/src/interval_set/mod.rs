// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

mod insert;
mod intersection;
pub mod interval;
mod remove;

#[cfg(test)]
mod tests;

use alloc::collections::vec_deque::{self, VecDeque};
use core::{
    fmt,
    iter::FromIterator,
    num::NonZeroUsize,
    ops::{Bound, Range, RangeBounds, RangeInclusive},
};
use insert::insert;
pub use intersection::Intersection;
pub use interval::*;
use remove::remove;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum IntervalSetError {
    LimitExceeded,
    InvalidInterval,
}

/// `IntervalSet` is an efficient structure for storing sets of consecutive numbers. Instead
/// of storing an individual entry per value, only the lower and upper bounds (`Interval`) are stored.
///
/// ## Usage
///
/// ```rust,ignore
/// use s2n_quic_transport::interval_set::IntervalSet;
///
/// let mut set = IntervalSet::new();
///
/// set.insert_value(0);
/// set.insert_value(1);
/// set.insert_value(2);
/// set.insert_value(3);
///
/// // because 0 to 3 are consecutive, only a single interval is stored
/// assert_eq!(set.interval_len(), 1);
///
/// set.insert_value(5);
/// set.insert_value(6);
///
/// // 5 and 6 are not consecutive with 0 to 3 so a new entry is created
/// assert_eq!(set.interval_len(), 2);
///
/// set.insert_value(4);
///
/// // inserting a 4 merges all of the intervals into a single entry
/// assert_eq!(set.interval_len(), 1);
///
/// // ranges of numbers can be inserted at the same time
/// set.insert(12..15);
/// set.insert(18..=21);
///
/// assert_eq!(set.interval_len(), 3);
///
/// // ranges can also be removed
/// set.remove(0..22);
///
/// assert_eq!(set.interval_len(), 0);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntervalSet<T> {
    limit: Option<NonZeroUsize>,
    intervals: VecDeque<Interval<T>>,
}

impl<T> Default for IntervalSet<T> {
    fn default() -> Self {
        Self {
            limit: None,
            intervals: VecDeque::new(),
        }
    }
}

impl<T> IntervalSet<T> {
    /// Creates an empty `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(0..4).is_ok());
    /// ```
    #[inline]
    pub fn new() -> IntervalSet<T> {
        Self::default()
    }

    /// Creates an empty `IntervalSet` with a specific capacity.
    /// This preallocates enough memory for `capacity` elements,
    /// so that the `IntervalSet` does not have to be reallocated
    /// until it contains at least that many values.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::with_capacity(10);
    /// assert!(set.insert(0..4).is_ok());
    /// ```
    #[inline]
    pub fn with_capacity(capacity: usize) -> IntervalSet<T> {
        let intervals = VecDeque::with_capacity(capacity);

        IntervalSet {
            limit: None,
            intervals,
        }
    }

    /// Creates an empty `IntervalSet` with a specific limit.
    /// The number of elements in the set cannot exceed this
    /// amount, otherwise `insert` calls will be rejected.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// use core::num::NonZeroUsize;
    /// let mut set = IntervalSet::with_limit(NonZeroUsize::new(1).unwrap());
    /// assert!(set.insert(0..4).is_ok());
    /// assert!(set.insert(12..16).is_err());
    /// assert!(set.insert(4..12).is_ok());
    /// assert!(set.insert(12..16).is_ok());
    /// ```
    #[inline]
    pub fn with_limit(limit: NonZeroUsize) -> IntervalSet<T> {
        let mut set = Self::default();
        set.set_limit(limit);
        set
    }

    /// Sets an element limit for the given `IntervalSet`.
    /// The number of elements in the set cannot exceed this
    /// amount, otherwise `insert` calls will be rejected
    ///
    /// Note: calling this will not drop existing intervals
    /// that exceed the new limit and will only be
    /// applied to later calls to `insert`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// use core::num::NonZeroUsize;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(0..4).is_ok());
    /// set.set_limit(NonZeroUsize::new(1).unwrap());
    /// assert!(set.insert(4..8).is_ok());
    /// assert!(set.insert(12..16).is_err());
    /// ```
    #[inline]
    pub fn set_limit(&mut self, limit: NonZeroUsize) {
        self.limit = Some(limit);
    }

    /// Removes the element limit for the given `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// use core::num::NonZeroUsize;
    /// let mut set = IntervalSet::with_limit(NonZeroUsize::new(1).unwrap());
    /// assert!(set.insert(0..4).is_ok());
    /// assert!(set.insert(4..8).is_ok());
    /// assert!(set.insert(12..16).is_err());
    /// set.remove_limit();
    /// assert!(set.insert(12..16).is_ok());
    /// ```
    #[inline]
    pub fn remove_limit(&mut self) {
        self.limit = None;
    }

    /// Returns the number of intervals in `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.interval_len(), 0);
    /// assert!(set.insert(0..4).is_ok());
    /// assert_eq!(set.interval_len(), 1);
    /// ```
    #[inline]
    pub fn interval_len(&self) -> usize {
        self.intervals.len()
    }

    /// Returns the allocated capacity of the `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::with_capacity(1);
    /// assert_eq!(set.capacity(), 1);
    /// assert!(set.insert(0..4).is_ok());
    /// assert!(set.insert(6..10).is_ok());
    /// assert!(set.capacity() > 1);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.intervals.capacity()
    }

    /// Clears all elements contained in the `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(0..4).is_ok());
    /// set.clear();
    /// assert!(set.is_empty());
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.intervals.clear()
    }

    /// Removes the lowest `Interval` in the set, if any
    ///
    /// # Examples
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.pop_min(), None);
    /// assert!(set.insert(0..4).is_ok());
    /// assert_eq!(set.pop_min(), Some((0..4).into()));
    /// ```
    #[inline]
    pub fn pop_min(&mut self) -> Option<Interval<T>> {
        self.intervals.pop_front()
    }
}

impl<T: IntervalBound> IntervalSet<T> {
    /// Returns the number of elements in `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.count(), 0);
    /// assert!(set.insert(0..4).is_ok());
    /// assert_eq!(set.count(), 4);
    /// ```
    #[inline]
    pub fn count(&self) -> usize {
        self.intervals.iter().map(|interval| interval.len()).sum()
    }

    /// Returns `true` if the `IntervalSet` has no intervals.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.is_empty());
    /// assert!(set.insert(0..4).is_ok());
    /// assert!(!set.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    /// Inserts the supplied `interval` into the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(0..4).is_ok());
    /// assert!(set.contains(&3));
    /// assert!(!set.contains(&5));
    /// ```
    #[inline]
    pub fn insert<R: RangeBounds<T>>(&mut self, interval: R) -> Result<(), IntervalSetError> {
        let interval = Interval::from_range_bounds(interval)?;

        if self.intervals.is_empty() {
            self.intervals.push_front(interval);
            return Ok(());
        }

        let index = self.index_for(&interval);
        insert(&mut self.intervals, interval, index, self.limit)?;

        self.check_integrity();

        Ok(())
    }

    /// Inserts the supplied `interval` at the beginning of the `IntervalSet`.
    /// This method can be used to optimize insertion when the `interval` is less
    /// than all of the current intervals.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert_front(0..4).is_ok());
    /// assert!(set.contains(&3));
    /// assert!(!set.contains(&5));
    /// ```
    #[inline]
    pub fn insert_front<R: RangeBounds<T>>(&mut self, interval: R) -> Result<(), IntervalSetError> {
        let interval = Interval::from_range_bounds(interval)?;

        if self.intervals.is_empty() {
            self.intervals.push_front(interval);
            return Ok(());
        }

        insert(&mut self.intervals, interval, 0, self.limit)?;

        self.check_integrity();

        Ok(())
    }

    /// Inserts a single `value` into the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert_value(1).is_ok());
    /// assert!(set.contains(&1));
    /// assert!(!set.contains(&0));
    /// assert!(!set.contains(&2));
    /// ```
    #[inline]
    pub fn insert_value(&mut self, value: T) -> Result<(), IntervalSetError> {
        self.insert((Bound::Included(value), Bound::Included(value)))
    }

    /// Performs a union, i.e., all the values in `self` or `other` will
    /// be present in `self`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut a = IntervalSet::new();
    /// assert!(a.insert(0..4).is_ok());
    /// let mut b = IntervalSet::new();
    /// assert!(b.insert(4..8).is_ok());
    /// a.union(&b);
    /// assert_eq!(a.iter().collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
    /// ```
    #[inline]
    pub fn union(&mut self, other: &Self) -> Result<(), IntervalSetError> {
        if self.intervals.is_empty() {
            self.intervals.clone_from(&other.intervals);
            return Ok(());
        }

        self.set_operation(other, insert)
    }

    /// Removes the supplied `interval` from the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(1..3).is_ok());
    /// assert!(set.remove(0..4).is_ok());
    /// assert!(set.is_empty());
    /// ```
    #[inline]
    pub fn remove<R: RangeBounds<T>>(&mut self, interval: R) -> Result<(), IntervalSetError> {
        let interval = Interval::from_range_bounds(interval)?;

        if self.intervals.is_empty() {
            return Ok(());
        }

        let index = self.index_for(&interval);
        remove(&mut self.intervals, interval, index, self.limit)?;

        self.check_integrity();

        Ok(())
    }

    /// Removes a single `value` from the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert_value(1).is_ok());
    /// assert!(set.remove_value(1).is_ok());
    /// assert!(!set.contains(&1));
    /// ```
    #[inline]
    pub fn remove_value(&mut self, value: T) -> Result<(), IntervalSetError> {
        self.remove((Bound::Included(value), Bound::Included(value)))
    }

    /// Performs a difference, i.e., all the values that are in `self` but not
    /// in `other` will be present in `self`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set_a = IntervalSet::new();
    /// assert!(set_a.insert(0..=10).is_ok());
    /// let mut set_b = IntervalSet::new();
    /// assert!(set_b.insert(4..=8).is_ok());
    /// assert!(set_a.difference(&set_b).is_ok());
    /// assert_eq!(set_a.iter().collect::<Vec<_>>(), vec![0, 1, 2, 3, 9, 10]);
    /// ```
    #[inline]
    pub fn difference(&mut self, other: &Self) -> Result<(), IntervalSetError> {
        if self.intervals.is_empty() {
            return Ok(());
        }

        self.set_operation(other, remove)
    }

    /// Performs an intersection, i.e., all the values in both `self` and `other` will
    /// be present in `self`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set_a = IntervalSet::new();
    /// assert!(set_a.insert(0..=10).is_ok());
    /// let mut set_b = IntervalSet::new();
    /// assert!(set_b.insert(4..=8).is_ok());
    /// assert!(set_a.intersection(&set_b).is_ok());
    /// assert_eq!(set_a.iter().collect::<Vec<_>>(), vec![4, 5, 6, 7, 8]);
    /// ```
    #[inline]
    pub fn intersection(&mut self, other: &Self) -> Result<(), IntervalSetError> {
        intersection::apply(&mut self.intervals, &other.intervals);

        Ok(())
    }

    /// Returns an iterator of `Intervals` over the intersection, i.e., all
    /// the values in both `self` and `other` will be returned.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set_a = IntervalSet::new();
    /// assert!(set_a.insert(0..=10).is_ok());
    /// let mut set_b = IntervalSet::new();
    /// assert!(set_b.insert(4..=8).is_ok());
    /// let intersection = set_a.intersection_iter(&set_b).flatten();
    /// assert_eq!(intersection.collect::<Vec<_>>(), vec![4, 5, 6, 7, 8]);
    /// ```
    #[inline]
    pub fn intersection_iter<'a>(&'a self, other: &'a Self) -> Intersection<'a, T> {
        Intersection::new(&self.intervals, &other.intervals)
    }

    /// Returns an iterator over all of the values contained in the given `IntervalSet`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert!(set.insert(0..5).is_ok());
    /// assert!(set.insert(10..15).is_ok());
    /// let items: Vec<_> = set.iter().collect();
    /// assert_eq!(vec![0, 1, 2, 3, 4, 10, 11, 12, 13, 14], items);
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.intervals.iter(),
            head: None,
            tail: None,
        }
    }

    /// Returns the smallest value in the given `IntervalSet`. If no items
    /// are present in the set, `None` is returned.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.min_value(), None);
    /// assert!(set.insert(0..5).is_ok());
    /// assert_eq!(set.min_value(), Some(0));
    /// ```
    #[inline]
    pub fn min_value(&self) -> Option<T> {
        let interval = self.intervals.front()?;
        Some(interval.start)
    }

    /// Returns the largest value in the given `IntervalSet`. If no items
    /// are present in the set, `None` is returned.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.max_value(), None);
    /// assert!(set.insert(0..5).is_ok());
    /// assert_eq!(set.max_value(), Some(4));
    /// ```
    #[inline]
    pub fn max_value(&self) -> Option<T> {
        let interval = self.intervals.back()?;
        Some(interval.end)
    }

    /// Returns `true` if the set contains a value
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// assert_eq!(set.contains(&1), false);
    /// assert!(set.insert(0..5).is_ok());
    /// assert_eq!(set.contains(&1), true);
    /// ```
    #[inline]
    pub fn contains(&self, value: &T) -> bool {
        self.binary_search_with(value, |_index| true, |_index| false, |_index| false)
    }

    /// Internal function for applying set operations
    #[inline]
    fn set_operation<
        F: Fn(
            &mut VecDeque<Interval<T>>,
            Interval<T>,
            usize,
            Option<NonZeroUsize>,
        ) -> Result<usize, IntervalSetError>,
    >(
        &mut self,
        other: &Self,
        apply: F,
    ) -> Result<(), IntervalSetError> {
        let mut iter = other.intervals.iter();
        let limit = self.limit;

        // get the first interval in `other` and find the applicable
        // index in `self`
        let interval = if let Some(interval) = iter.next() {
            interval
        } else {
            return Ok(());
        };

        let mut index = self.index_for(interval);

        // apply the set operation for the interval above
        index = apply(&mut self.intervals, *interval, index, limit)?;

        // apply the set operation for the rest of the intervals
        for interval in iter {
            index = apply(&mut self.intervals, *interval, index, limit)?;
        }

        self.check_integrity();

        Ok(())
    }

    /// Internal function for locating the optimal starting index for
    /// interval comparison
    #[inline]
    fn index_for(&self, interval: &Interval<T>) -> usize {
        // it's faster just to iterate through the set for smaller lengths
        if self.interval_len() < 16 {
            return 0;
        }

        self.binary_search_with(&interval.start, |index| index, |index| index, |index| index)
    }

    /// Internal function for searching for a value in the contained intervals
    #[inline]
    fn binary_search_with<
        V,
        EqualFn: Fn(usize) -> V,
        GreaterFn: Fn(usize) -> V,
        LessFn: Fn(usize) -> V,
    >(
        &self,
        value: &T,
        on_equal: EqualFn,
        on_greater: GreaterFn,
        on_less: LessFn,
    ) -> V {
        use core::cmp::Ordering::*;

        let intervals = &self.intervals;

        let mut size = intervals.len();
        if size == 0 {
            return on_greater(0);
        }

        let mut base = 0usize;
        while size > 1 {
            let half = size / 2;
            let mid = base + half;
            let subject = &intervals[mid];
            match subject.partial_cmp(value) {
                Some(Equal) => return on_equal(mid),
                Some(Greater) => {}
                Some(Less) => base = mid,
                None => return on_greater(0),
            };
            size -= half;
        }

        let subject = &intervals[base];
        match subject.partial_cmp(value) {
            Some(Equal) => on_equal(base),
            Some(Greater) => on_greater(base),
            Some(Less) => on_less(base),
            None => on_greater(0),
        }
    }

    /// Internal check for integrity - only used when `cfg(test)` is enabled
    #[inline]
    fn check_integrity(&self) {
        // When using this data structure outside of this crate, these checks are quite expensive.
        // Rather than using `cfg(debug_assertions)`, we limit it to `cfg(test)`, which will just
        // turn them on when testing this crate.
        if cfg!(test) {
            let mut prev_end: Option<T> = None;

            for interval in self.intervals.iter() {
                // make sure that a few items exist
                for value in (*interval).take(3) {
                    assert!(self.contains(&value), "set should contain value");
                }
                for value in (*interval).rev().take(3) {
                    assert!(self.contains(&value), "set should contain value");
                }

                if let Some(prev_end) = prev_end.as_ref() {
                    assert!(
                        *prev_end < interval.start_exclusive(),
                        "the previous end should be less than the next start",
                    );
                }

                assert!(interval.is_valid(), "interval should be valid");

                prev_end = Some(interval.end);
            }
        }
    }
}

impl<T: Copy> IntervalSet<T> {
    /// Returns an iterator of `Interval`s contained in the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// set.insert(0..=10);
    /// assert_eq!(set.intervals().collect::<Vec<_>>(), vec![0..=10]);
    /// ```
    #[inline]
    pub fn intervals(&self) -> IntervalIter<'_, T> {
        IntervalIter {
            iter: self.intervals.iter(),
        }
    }

    /// Returns an iterator of `Range`s contained in the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// set.insert(0..=10);
    /// assert_eq!(set.ranges().collect::<Vec<_>>(), vec![0..11]);
    /// ```
    #[inline]
    pub fn ranges(&self) -> RangeIter<'_, T> {
        RangeIter {
            iter: self.intervals.iter(),
        }
    }

    /// Returns an iterator of `RangeInclusive`s contained in the `IntervalSet`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use s2n_quic_transport::interval_set::IntervalSet;
    /// let mut set = IntervalSet::new();
    /// set.insert(0..=10);
    /// assert_eq!(set.inclusive_ranges().collect::<Vec<_>>(), vec![0..=10]);
    /// ```
    #[inline]
    pub fn inclusive_ranges(&self) -> RangeInclusiveIter<'_, T> {
        RangeInclusiveIter {
            iter: self.intervals.iter(),
        }
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for IntervalSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_set().entries(self.intervals.iter()).finish()
    }
}

/// Iterator over all of the values contained in an `IntervalSet`
pub struct Iter<'a, T> {
    iter: vec_deque::Iter<'a, Interval<T>>,
    head: Option<Interval<T>>,
    tail: Option<Interval<T>>,
}

impl<T: IntervalBound> Iterator for Iter<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        loop {
            if let Some(item) = self.head.as_mut().and_then(Iterator::next) {
                return Some(item);
            }

            let item = self.iter.next().cloned().or_else(|| self.tail.take())?;
            self.head = Some(item);
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, _) = self.iter.size_hint();
        // Computing the upper length would require iterating through all of the ranges
        let upper = None;
        (lower, upper)
    }
}

impl<T: IntervalBound> DoubleEndedIterator for Iter<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        loop {
            if let Some(item) = self.tail.as_mut().and_then(DoubleEndedIterator::next_back) {
                return Some(item);
            }

            let item = self
                .iter
                .next_back()
                .cloned()
                .or_else(|| self.head.take())?;
            self.tail = Some(item);
        }
    }
}

macro_rules! impl_iterator_conversions {
    ($item:ident, $iter:ident) => {
        #[derive(Clone, Debug)]
        pub struct $iter<'a, T> {
            iter: vec_deque::Iter<'a, Interval<T>>,
        }

        impl<'a, T: IntervalBound> Iterator for $iter<'a, T> {
            type Item = $item<T>;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                self.iter.next().map(|interval| interval.into())
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                self.iter.size_hint()
            }
        }

        impl<'a, T: IntervalBound> DoubleEndedIterator for $iter<'a, T> {
            #[inline]
            fn next_back(&mut self) -> Option<Self::Item> {
                self.iter.next_back().map(|interval| interval.into())
            }
        }

        impl<'a, T: IntervalBound> ExactSizeIterator for $iter<'a, T> where
            vec_deque::Iter<'a, Interval<T>>: ExactSizeIterator
        {
        }

        impl<T: IntervalBound> FromIterator<$item<T>> for IntervalSet<T> {
            #[inline]
            fn from_iter<I: IntoIterator<Item = $item<T>>>(intervals: I) -> Self {
                let intervals = intervals.into_iter();
                let mut set = Self::with_capacity(intervals.size_hint().0);
                set.extend(intervals);
                set
            }
        }

        impl<'a, T: 'a + IntervalBound> FromIterator<&'a $item<T>> for IntervalSet<T> {
            #[inline]
            fn from_iter<I: IntoIterator<Item = &'a $item<T>>>(intervals: I) -> Self {
                let intervals = intervals.into_iter();
                let mut set = Self::with_capacity(intervals.size_hint().0);
                set.extend(intervals);
                set
            }
        }

        impl<T: IntervalBound> Extend<$item<T>> for IntervalSet<T> {
            #[inline]
            fn extend<I: IntoIterator<Item = $item<T>>>(&mut self, intervals: I) {
                for interval in intervals {
                    if self.insert(interval).is_err() {
                        return;
                    }
                }
            }
        }

        impl<'a, T: 'a + IntervalBound> Extend<&'a $item<T>> for IntervalSet<T> {
            #[inline]
            fn extend<I: IntoIterator<Item = &'a $item<T>>>(&mut self, intervals: I) {
                for interval in intervals {
                    let interval: Interval<T> = interval.into();
                    if self.insert(interval).is_err() {
                        return;
                    }
                }
            }
        }

        impl<T: IntervalBound> From<$item<T>> for IntervalSet<T> {
            #[inline]
            fn from(interval: $item<T>) -> Self {
                let mut set = Self::with_capacity(1);
                let _ = set.insert(interval);
                set
            }
        }
    };
}

impl_iterator_conversions!(Interval, IntervalIter);
impl_iterator_conversions!(Range, RangeIter);
impl_iterator_conversions!(RangeInclusive, RangeInclusiveIter);

impl<T: IntervalBound> FromIterator<T> for IntervalSet<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        let values = values.into_iter();
        let mut set = Self::with_capacity(values.size_hint().0);
        for value in values {
            if set.insert_value(value).is_err() {
                break;
            }
        }
        set
    }
}

impl<'a, T: 'a + IntervalBound> FromIterator<&'a T> for IntervalSet<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = &'a T>>(values: I) -> Self {
        values.into_iter().cloned().collect()
    }
}
