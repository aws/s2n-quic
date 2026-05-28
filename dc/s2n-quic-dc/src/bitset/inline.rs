// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::BitSet64;
use core::fmt;

/// A fixed-size inline bitset backed by `[BitSet64; N]`.
///
/// Supports up to `N * 64` bits with no heap allocation. All operations are O(N) or better.
/// Typical use: `InlineBitSet<4>` for 256 bits (covers up to 256 chunks / 2MB at 8KB/chunk).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InlineBitSet<const N: usize> {
    words: [BitSet64; N],
}

impl<const N: usize> Default for InlineBitSet<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> fmt::Debug for InlineBitSet<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<const N: usize> InlineBitSet<N> {
    pub const CAPACITY: u32 = N as u32 * 64;

    pub const fn new() -> Self {
        Self {
            words: [BitSet64::new(); N],
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.words == [BitSet64::new(); N]
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.words == [BitSet64::all(); N]
    }

    /// Returns true if all bits in `0..count` are set.
    #[inline]
    pub fn all_set(&self, count: u32) -> bool {
        debug_assert!(count <= Self::CAPACITY);

        let full_words = (count / 64) as usize;
        let remainder = (count % 64) as u8;

        if self.words[..full_words] != [BitSet64::all(); N][..full_words] {
            return false;
        }

        if remainder > 0 && full_words < N {
            let expected = BitSet64::from(0..=(remainder - 1));
            return self.words[full_words].mask(expected) == expected;
        }

        true
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.words.iter().map(|w| w.len()).sum()
    }

    #[inline]
    pub fn get(&self, index: u32) -> bool {
        debug_assert!(index < Self::CAPACITY);
        let word = (index / 64) as usize;
        let bit = (index % 64) as u8;
        self.words[word].get(bit)
    }

    /// Sets the bit at `index`. Returns true if the bit was newly inserted (was previously clear).
    #[inline]
    pub fn insert(&mut self, index: u32) -> bool {
        debug_assert!(index < Self::CAPACITY);
        let word = (index / 64) as usize;
        let bit = (index % 64) as u8;
        self.words[word].insert(bit)
    }

    /// Clears the bit at `index`. Returns true if the bit was previously set.
    #[inline]
    pub fn remove(&mut self, index: u32) -> bool {
        debug_assert!(index < Self::CAPACITY);
        let word = (index / 64) as usize;
        let bit = (index % 64) as u8;
        self.words[word].remove(bit)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.words = [BitSet64::new(); N];
    }

    pub fn iter(&self) -> Iter<'_, N> {
        Iter {
            words: &self.words,
            word_index: 0,
            current: if N > 0 {
                self.words[0].iter()
            } else {
                BitSet64::new().iter()
            },
        }
    }
}

pub struct Iter<'a, const N: usize> {
    words: &'a [BitSet64; N],
    word_index: usize,
    current: super::bitset64::Iter,
}

impl<const N: usize> Iterator for Iter<'_, N> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(bit) = self.current.next() {
                return Some(self.word_index as u32 * 64 + bit as u32);
            }
            self.word_index += 1;
            if self.word_index >= N {
                return None;
            }
            self.current = self.words[self.word_index].iter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let bs = InlineBitSet::<4>::new();
        assert!(bs.is_empty());
        assert!(!bs.is_full());
        assert_eq!(bs.len(), 0);
        assert_eq!(InlineBitSet::<4>::CAPACITY, 256);
    }

    #[test]
    fn insert_and_get() {
        let mut bs = InlineBitSet::<4>::new();
        assert!(bs.insert(0));
        assert!(!bs.insert(0));
        assert!(bs.get(0));
        assert!(!bs.get(1));

        assert!(bs.insert(63));
        assert!(bs.insert(64));
        assert!(bs.insert(127));
        assert!(bs.insert(128));
        assert!(bs.insert(255));

        assert_eq!(bs.len(), 6);
        assert!(bs.get(63));
        assert!(bs.get(64));
        assert!(bs.get(127));
        assert!(bs.get(128));
        assert!(bs.get(255));
    }

    #[test]
    fn remove() {
        let mut bs = InlineBitSet::<4>::new();
        bs.insert(100);
        assert!(bs.remove(100));
        assert!(!bs.remove(100));
        assert!(!bs.get(100));
        assert!(bs.is_empty());
    }

    #[test]
    fn full() {
        let mut bs = InlineBitSet::<1>::new();
        for i in 0..64 {
            bs.insert(i);
        }
        assert!(bs.is_full());
        assert_eq!(bs.len(), 64);
    }

    #[test]
    fn all_set() {
        let mut bs = InlineBitSet::<4>::new();

        assert!(bs.all_set(0));

        for i in 0..128 {
            bs.insert(i);
        }
        assert!(bs.all_set(128));
        assert!(!bs.all_set(129));

        for i in 128..256 {
            bs.insert(i);
        }
        assert!(bs.all_set(256));
    }

    #[test]
    fn all_set_partial_word() {
        let mut bs = InlineBitSet::<4>::new();
        for i in 0..100 {
            bs.insert(i);
        }
        assert!(bs.all_set(100));
        assert!(!bs.all_set(101));
    }

    #[test]
    fn clear() {
        let mut bs = InlineBitSet::<4>::new();
        for i in 0..256 {
            bs.insert(i);
        }
        bs.clear();
        assert!(bs.is_empty());
        assert_eq!(bs.len(), 0);
    }

    #[test]
    fn iter() {
        let mut bs = InlineBitSet::<4>::new();
        bs.insert(0);
        bs.insert(63);
        bs.insert(64);
        bs.insert(200);
        bs.insert(255);

        let collected: Vec<u32> = bs.iter().collect();
        assert_eq!(collected, vec![0, 63, 64, 200, 255]);
    }

    #[test]
    fn iter_empty() {
        let bs = InlineBitSet::<4>::new();
        assert_eq!(bs.iter().count(), 0);
    }

    #[test]
    fn different_sizes() {
        let bs1 = InlineBitSet::<1>::new();
        assert_eq!(InlineBitSet::<1>::CAPACITY, 64);
        assert!(bs1.is_empty());

        let bs2 = InlineBitSet::<2>::new();
        assert_eq!(InlineBitSet::<2>::CAPACITY, 128);
        assert!(bs2.is_empty());

        let bs8 = InlineBitSet::<8>::new();
        assert_eq!(InlineBitSet::<8>::CAPACITY, 512);
        assert!(bs8.is_empty());
    }

    #[test]
    fn word_boundaries() {
        let mut bs = InlineBitSet::<4>::new();
        // Test bits at every word boundary
        for boundary in [0, 63, 64, 127, 128, 191, 192, 255] {
            bs.insert(boundary);
        }
        assert_eq!(bs.len(), 8);
        for boundary in [0, 63, 64, 127, 128, 191, 192, 255] {
            assert!(bs.get(boundary), "bit {boundary} should be set");
        }
    }
}
