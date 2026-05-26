// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct BitSet64(u64);

impl fmt::Debug for BitSet64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl BitSet64 {
    pub const fn new() -> Self {
        Self(0)
    }

    pub const fn all() -> Self {
        Self(!0)
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn is_full(&self) -> bool {
        self.0 == !0
    }

    #[inline]
    pub const fn len(&self) -> u32 {
        self.0.count_ones()
    }

    #[inline]
    pub const fn get(&self, index: u8) -> bool {
        debug_assert!(index < 64);
        (self.0 & (1 << index)) != 0
    }

    #[inline]
    pub fn insert(&mut self, index: u8) -> bool {
        debug_assert!(index < 64);
        let newly_inserted = !self.get(index);
        self.0 |= 1 << index;
        newly_inserted
    }

    #[inline]
    pub fn remove(&mut self, index: u8) -> bool {
        debug_assert!(index < 64);
        let was_present = self.get(index);
        self.0 &= !(1 << index);
        was_present
    }

    #[inline]
    pub const fn first(&self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            Some(self.0.trailing_zeros() as u8)
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    pub fn iter(&self) -> Iter {
        Iter { remaining: self.0 }
    }
}

impl From<RangeInclusive<u8>> for BitSet64 {
    fn from(value: RangeInclusive<u8>) -> Self {
        let min = *value.start();
        let max = *value.end();
        debug_assert!(min <= max);
        debug_assert!(max < 64);

        let min_mask = if min == 0 { 0 } else { (1u64 << min) - 1 };
        let max_mask = if max == 63 {
            u64::MAX
        } else {
            (1u64 << (max + 1)) - 1
        };

        BitSet64(!min_mask & max_mask)
    }
}

pub struct Iter {
    remaining: u64,
}

impl Iterator for Iter {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let index = self.remaining.trailing_zeros() as u8;
        self.remaining &= self.remaining - 1;
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_operations() {
        let mut bitset = BitSet64::new();
        assert!(bitset.is_empty());
        assert_eq!(bitset.len(), 0);
        assert_eq!(bitset.first(), None);

        bitset.insert(0);
        bitset.insert(10);
        bitset.insert(63);

        assert!(!bitset.is_empty());
        assert_eq!(bitset.len(), 3);
        assert!(bitset.get(0));
        assert!(bitset.get(10));
        assert!(bitset.get(63));
        assert!(!bitset.get(1));
        assert_eq!(bitset.first(), Some(0));

        bitset.remove(0);
        assert!(!bitset.get(0));
        assert_eq!(bitset.len(), 2);
        assert_eq!(bitset.first(), Some(10));

        bitset.clear();
        assert!(bitset.is_empty());
    }

    #[test]
    fn from_range_inclusive() {
        let bitset = BitSet64::from(10..=20);
        assert_eq!(bitset.len(), 11);
        assert!(bitset.get(10));
        assert!(bitset.get(20));
        assert!(!bitset.get(9));
        assert!(!bitset.get(21));

        let bitset = BitSet64::from(0..=63);
        assert!(bitset.is_full());

        let bitset = BitSet64::from(0..=0);
        assert_eq!(bitset.len(), 1);
        assert!(bitset.get(0));
    }

    #[test]
    fn iterator() {
        let mut bitset = BitSet64::new();
        bitset.insert(5);
        bitset.insert(10);
        bitset.insert(32);
        bitset.insert(63);
        let collected: Vec<u8> = bitset.iter().collect();
        assert_eq!(collected, vec![5, 10, 32, 63]);
    }
}
