// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace};
use alloc::collections::VecDeque;
use core::fmt;

/// A packet number map backed by a sorted VecDeque of (PacketNumber, value) pairs.
///
/// All operations are O(log n) or O(k) where n is the number of stored entries
/// and k is the number of entries affected. This avoids the O(span) pathology
/// of the ring-buffer map when entries are sparse relative to the PN space.
///
/// Uses VecDeque so that front removals (the common case for ACK processing and
/// loss detection) are O(1) amortized — drain from the front just advances the
/// head pointer with no data movement.
///
/// No hashmap randomization — just deterministic binary search over a sorted ring.
#[derive(Clone)]
pub struct SortedVecMap<V> {
    entries: VecDeque<(PacketNumber, V)>,
}

impl<V: fmt::Debug> fmt::Debug for SortedVecMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(pn, v)| (pn, v)))
            .finish()
    }
}

impl<V> Default for SortedVecMap<V> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }
}

impl<V> SortedVecMap<V> {
    /// Inserts the given `value` at `packet_number`.
    ///
    /// Packet numbers must be inserted in monotonically increasing order.
    #[inline]
    pub fn insert(&mut self, packet_number: PacketNumber, value: V) {
        debug_assert!(
            self.entries.back().is_none_or(|(last, _)| packet_number > *last),
            "packet numbers must be monotonically increasing"
        );
        self.entries.push_back((packet_number, value));
    }

    /// Inserts the given `value` or updates the existing entry.
    #[inline]
    pub fn insert_or_update<F: FnOnce(&mut V)>(
        &mut self,
        packet_number: PacketNumber,
        value: V,
        update: F,
    ) {
        match self.search(packet_number) {
            Ok(idx) => update(&mut self.entries[idx].1),
            Err(idx) => self.entries.insert(idx, (packet_number, value)),
        }
    }

    /// Returns a reference to the value at `packet_number`.
    #[inline]
    pub fn get(&self, packet_number: PacketNumber) -> Option<&V> {
        self.search(packet_number)
            .ok()
            .map(|idx| &self.entries[idx].1)
    }

    /// Returns a mutable reference to the value at `packet_number`.
    #[inline]
    pub fn get_mut(&mut self, packet_number: PacketNumber) -> Option<&mut V> {
        self.search(packet_number)
            .ok()
            .map(|idx| &mut self.entries[idx].1)
    }

    /// Removes the value at `packet_number`.
    #[inline]
    pub fn remove(&mut self, packet_number: PacketNumber) -> Option<V> {
        self.search(packet_number)
            .ok()
            .map(|idx| self.entries.remove(idx).unwrap().1)
    }

    /// Removes all entries in the given range and returns an iterator over them.
    ///
    /// Cost: O(log n + k) where k is the number of entries actually in the range.
    /// Front removals (the common case) shift nothing; VecDeque::drain moves the
    /// shorter side.
    #[inline]
    pub fn remove_range(&mut self, range: PacketNumberRange) -> RemoveIter<'_, V> {
        let start_pn = range.start();
        let end_pn = range.end();

        let start_idx = self.partition_point(start_pn);
        let end_idx = start_idx + self.partition_point_from(start_idx, end_pn);

        RemoveIter {
            drain: self.entries.drain(start_idx..end_idx),
        }
    }

    /// Get the inclusive PacketNumberRange covering all entries.
    #[inline]
    pub fn get_range(&self) -> PacketNumberRange {
        if self.entries.is_empty() {
            let base = PacketNumberSpace::Initial.new_packet_number(0u8.into());
            return PacketNumberRange::new(base, base);
        }
        let start = self.entries.front().unwrap().0;
        let end = self.entries.back().unwrap().0;
        PacketNumberRange::new(start, end)
    }

    /// Gets an iterator over entries, sorted by PacketNumber.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (PacketNumber, &V)> {
        self.entries.iter().map(|(pn, v)| (*pn, v))
    }

    /// Gets a mutable iterator over entries, sorted by PacketNumber.
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (PacketNumber, &mut V)> {
        self.entries.iter_mut().map(|(pn, v)| (*pn, v))
    }

    /// Returns true if there are no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all entries.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[inline]
    fn search(&self, packet_number: PacketNumber) -> Result<usize, usize> {
        self.entries
            .binary_search_by_key(&packet_number, |(pn, _)| *pn)
    }

    /// Find the first index where pn >= target.
    #[inline]
    fn partition_point(&self, target: PacketNumber) -> usize {
        self.entries.partition_point(|(pn, _)| *pn < target)
    }

    /// Find how many entries from `from` have pn <= target.
    #[inline]
    fn partition_point_from(&self, from: usize, target: PacketNumber) -> usize {
        // VecDeque doesn't have a slice-based partition_point from an offset,
        // so we use the range view.
        let len = self.entries.len() - from;
        let mut lo = 0;
        let mut hi = len;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.entries[from + mid].0 <= target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

/// Lazy iterator over removed entries from a range removal.
///
/// Wraps `VecDeque::drain` — items are yielded one at a time. Front removals
/// (the common path for ACK/loss) require no data movement at all.
pub struct RemoveIter<'a, V> {
    drain: alloc::collections::vec_deque::Drain<'a, (PacketNumber, V)>,
}

impl<V> Iterator for RemoveIter<'_, V> {
    type Item = (PacketNumber, V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.drain.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.drain.size_hint()
    }
}

impl<V> ExactSizeIterator for RemoveIter<'_, V> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        packet::number::{PacketNumberRange, PacketNumberSpace},
        varint::VarInt,
    };
    use alloc::collections::BTreeMap;
    use bolero::{check, generator::*};

    type TestMap = SortedVecMap<u64>;

    #[test]
    fn basic_insert_get_remove() {
        let space = PacketNumberSpace::Initial;
        let mut map = TestMap::default();
        let pn1 = space.new_packet_number(VarInt::from_u8(1));
        let pn2 = space.new_packet_number(VarInt::from_u8(2));
        let pn3 = space.new_packet_number(VarInt::from_u8(5));

        map.insert(pn1, 10);
        map.insert(pn2, 20);
        map.insert(pn3, 50);

        assert_eq!(map.get(pn1), Some(&10));
        assert_eq!(map.get(pn2), Some(&20));
        assert_eq!(map.get(pn3), Some(&50));

        assert_eq!(map.remove(pn2), Some(20));
        assert_eq!(map.get(pn2), None);
        assert_eq!(map.get(pn1), Some(&10));
        assert_eq!(map.get(pn3), Some(&50));
    }

    #[test]
    fn remove_range_basic() {
        let space = PacketNumberSpace::Initial;
        let mut map = TestMap::default();
        for i in 0u64..10 {
            let pn = space.new_packet_number(VarInt::new(i).unwrap());
            map.insert(pn, i);
        }

        let start = space.new_packet_number(VarInt::from_u8(3));
        let end = space.new_packet_number(VarInt::from_u8(7));
        let range = PacketNumberRange::new(start, end);

        let removed: Vec<_> = map.remove_range(range).collect();
        assert_eq!(removed.len(), 5);
        assert_eq!(removed[0].1, 3);
        assert_eq!(removed[4].1, 7);

        assert_eq!(map.iter().count(), 5);
    }

    #[test]
    fn remove_range_sparse() {
        let space = PacketNumberSpace::Initial;
        let mut map = TestMap::default();
        for i in 0u64..5 {
            let pn = space.new_packet_number(VarInt::new(i * 10).unwrap());
            map.insert(pn, i * 10);
        }

        let start = space.new_packet_number(VarInt::from_u8(5));
        let end = space.new_packet_number(VarInt::from_u8(35));
        let range = PacketNumberRange::new(start, end);

        let removed: Vec<_> = map.remove_range(range).collect();
        assert_eq!(removed.len(), 3);
        assert_eq!(removed[0].1, 10);
        assert_eq!(removed[1].1, 20);
        assert_eq!(removed[2].1, 30);
    }

    #[test]
    fn remove_front_is_efficient() {
        let space = PacketNumberSpace::Initial;
        let mut map = TestMap::default();
        for i in 0u64..100 {
            let pn = space.new_packet_number(VarInt::new(i).unwrap());
            map.insert(pn, i);
        }

        // Remove from the front (simulates loss detection)
        let start = space.new_packet_number(VarInt::from_u8(0));
        let end = space.new_packet_number(VarInt::from_u8(49));
        let range = PacketNumberRange::new(start, end);

        let removed: Vec<_> = map.remove_range(range).collect();
        assert_eq!(removed.len(), 50);
        assert_eq!(map.iter().count(), 50);
        // First remaining entry should be PN 50
        assert_eq!(map.entries.front().unwrap().0.as_u64(), 50);
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Operation {
        Insert,
        Skip,
        Remove(VarInt),
        RemoveRange(VarInt, VarInt),
        Clear,
    }

    fn model(ops: &[Operation]) {
        let space = PacketNumberSpace::ApplicationData;
        let mut current = space.new_packet_number(VarInt::from_u8(0));

        #[derive(Debug, Default)]
        struct Model {
            subject: SortedVecMap<u64>,
            oracle: BTreeMap<PacketNumber, u64>,
        }

        impl Model {
            pub fn insert(&mut self, packet_number: PacketNumber) {
                let value = packet_number.as_u64();
                self.subject.insert(packet_number, value);
                self.oracle.insert(packet_number, value);
                self.check_consistency();
            }

            pub fn remove(&mut self, packet_number: PacketNumber) {
                assert_eq!(
                    self.subject.remove(packet_number),
                    self.oracle.remove(&packet_number)
                );
                self.check_consistency();
            }

            pub fn remove_range(&mut self, range: PacketNumberRange) {
                // Trim range to actual bounds so the oracle loop doesn't iterate
                // over millions of PNs that can't possibly be in the map.
                let range = if self.subject.is_empty() {
                    PacketNumberRange::new(range.start(), range.start())
                } else {
                    let bounds = self.subject.get_range();
                    let start = range.start().max(bounds.start());
                    let end = range.end().min(bounds.end());
                    if start > end {
                        PacketNumberRange::new(start, start)
                    } else {
                        PacketNumberRange::new(start, end)
                    }
                };

                let actual: Vec<_> = self.subject.remove_range(range).collect();
                let mut expected = vec![];
                for pn in range {
                    if let Some(value) = self.oracle.remove(&pn) {
                        expected.push((pn, value));
                    }
                }
                assert_eq!(expected, actual);
                self.check_consistency();
            }

            pub fn clear(&mut self) {
                self.subject.clear();
                self.oracle.clear();
                self.check_consistency();
            }

            fn check_consistency(&self) {
                let mut subject = self.subject.iter();
                let mut oracle = self.oracle.iter();
                loop {
                    match (subject.next(), oracle.next()) {
                        (Some(actual), Some((expected_pn, expected_info))) => {
                            assert_eq!((*expected_pn, expected_info), actual);
                        }
                        (None, None) => break,
                        (actual, expected) => {
                            panic!("expected: {expected:?}, actual: {actual:?}");
                        }
                    }
                }
            }
        }

        let mut model = Model::default();

        for op in ops.iter().copied() {
            match op {
                Operation::Insert => {
                    model.insert(current);
                    current = current.next().unwrap();
                }
                Operation::Skip => {
                    current = current.next().unwrap();
                }
                Operation::Remove(pn) => {
                    let pn = space.new_packet_number(pn);
                    model.remove(pn);
                }
                Operation::RemoveRange(start, end) => {
                    let (start, end) = if start > end {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    let start = space.new_packet_number(start);
                    let end = space.new_packet_number(end);
                    let range = PacketNumberRange::new(start, end);
                    model.remove_range(range);
                }
                Operation::Clear => {
                    model.clear();
                }
            }
        }
    }

    #[test]
    fn differential_test() {
        check!()
            .with_type::<Vec<Operation>>()
            .with_iterations(1_000)
            .for_each(|ops| model(ops))
    }
}
