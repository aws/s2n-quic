// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace};
use alloc::{boxed::Box, vec::Vec};
use core::fmt;

/// A data structure for tracking packets that are pending acknowledgement
///
/// The following assumptions are made and exploited
///
/// * Packet numbers are monotonically generated and inserted
/// * Packet numbers will mostly be removed in ranges
/// * Packet numbers that are deemed lost will also be removed and retransmitted
///
/// This is implemented as a buffer ring with a moving range for the lower and upper bound of
/// contained packet numbers. The following example illustrates how each field tracks state:
///
/// ```ignore
/// packets = [ PN(2), None, PN(0), PN(1) ]
///                           ^ index = 2
/// start = PN(0)
/// end = PN(2)
/// ```
///
/// Upon inserting `PN(3)` the state is now:
///
/// ```ignore
/// packets = [ PN(2), PN(3), PN(0), PN(1) ]
///                           ^ index = 2
/// start = PN(0)
/// end = PN(3)
/// ```
///
/// Upon removing `PN(0)` the state is now:
///
/// ```ignore
/// packets = [ PN(2), PN(3), None, PN(1) ]
///                                 ^ index = 3
/// start = PN(1)
/// end = PN(3)
/// ```
#[derive(Clone)]
pub struct Map<V> {
    /// The stored values for each packet number
    values: Box<[Option<V>]>,
    /// The smallest contained inclusive packet number in the map
    start: PacketNumber,
    /// The largest contained inclusive packet number in the map
    end: PacketNumber,
    /// The starting index of the first occupied packet
    ///
    /// This field will be set to the `packets.len()` if the map is empty
    index: usize,
}

impl<V: fmt::Debug> fmt::Debug for Map<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Start with 8 sent packets at a time
///
/// Capacity will grow exponentially as more packet number entries are added
const DEFAULT_CAPACITY: usize = 8;

impl<V> Default for Map<V> {
    fn default() -> Self {
        // we use the Initial packet number space as a filler until an actual
        // packet number is inserted
        let base = PacketNumberSpace::Initial.new_packet_number(0u8.into());

        let mut values = Vec::with_capacity(DEFAULT_CAPACITY);
        while values.len() != values.capacity() {
            values.push(None);
        }
        let values = values.into_boxed_slice();

        // Set the index to the len (OOB) to indicate that it's empty
        let index = values.len();

        Self {
            values,
            start: base,
            end: base,
            index,
        }
    }
}

impl<V> Map<V> {
    /// Inserts the given `value`
    pub fn insert(&mut self, packet_number: PacketNumber, value: V) {
        if self.is_empty() {
            self.start = packet_number;
            self.end = packet_number;
            self.values[0] = Some(value);
            self.index = 0;
            return;
        }

        // The implementation assumes monotonicity of insertion
        debug_assert!(
            packet_number > self.start && packet_number > self.end,
            "packet numbers should be monotonic: {:?} > {:?} && {:?}",
            packet_number,
            self.start,
            self.end
        );

        // check if we need to increase capacity
        let distance = (packet_number.as_u64() - self.start.as_u64()) as usize;

        let index = if distance >= self.values.len() {
            self.resize(distance);

            // use the distance as the index since we've already resized beyond it
            distance
        } else {
            (self.index + distance) % self.values.len()
        };

        self.values[index] = Some(value);
        self.end = packet_number;
    }

    /// Inserts the given `value` into the map or updates the existing entry
    pub fn insert_or_update<F: FnOnce(&mut V)>(
        &mut self,
        packet_number: PacketNumber,
        value: V,
        update: F,
    ) {
        if self.is_empty() {
            self.start = packet_number;
            self.end = packet_number;
            self.values[0] = Some(value);
            self.index = 0;
            return;
        }

        // The implementation assumes insertion is not lower than the start
        debug_assert!(
            packet_number >= self.start,
            "packet numbers should be monotonic: {:?} > {:?}",
            packet_number,
            self.start,
        );

        // check if we need to increase capacity
        let distance = (packet_number.as_u64() - self.start.as_u64()) as usize;

        let index = if distance >= self.values.len() {
            self.resize(distance);

            // use the distance as the index since we've already resized beyond it
            distance
        } else {
            (self.index + distance) % self.values.len()
        };

        let entry = &mut self.values[index];

        if let Some(prev) = entry.as_mut() {
            update(prev);
        } else {
            *entry = Some(value);
        }

        self.end = self.end.max(packet_number);
    }

    /// Returns a reference to the `V` associated with the given `packet_number`
    #[inline]
    pub fn get(&self, packet_number: PacketNumber) -> Option<&V> {
        let index = self.pn_index(packet_number)?;
        self.values[index].as_ref()
    }

    /// Removes the value associated with the given `packet_number`
    /// and returns the value if it was present
    pub fn remove(&mut self, packet_number: PacketNumber) -> Option<V> {
        let index = self.pn_index(packet_number)?;
        let info = self.values[index].take()?;

        // update the bounds
        match (self.start == packet_number, self.end == packet_number) {
            // the bounds are inclusive so the map is now empty, reset it
            //              [_, _, _, 3]
            // remove(3) => [_, _, _, _]
            (true, true) => {
                self.clear();
            }
            // the packet was removed from the front
            //              [0, 1, _, 3, 4]
            // remove(0) => [_, 1, _, 3, 4]
            // remove(1) => [_, _, _, 3, 4]
            // remove(3) => [_, _, _, _, 4]
            (true, false) => {
                self.set_start(packet_number.next().unwrap());
            }
            // the packet was removed from the back
            //              [0, 1, _, 3, 4]
            // remove(4) => [0, 1, _, 3, _]
            // remove(3) => [0, 1, _, _, _]
            // remove(1) => [0, _, _, _, _]
            (false, true) => {
                self.set_end(packet_number.prev().unwrap());
            }
            // the packet was removed from the middle
            //              [0, 1, 2]
            // remove(2) => [0, _, 2]
            (false, false) => {
                // nothing to do
            }
        }

        Some(info)
    }

    /// Removes a range of packets from the map and returns their value
    #[inline]
    pub fn remove_range(&mut self, range: PacketNumberRange) -> RemoveIter<V> {
        RemoveIter::new(self, range)
    }

    /// Get the inclusive PacketNumberRange
    #[inline]
    pub fn get_range(&self) -> PacketNumberRange {
        PacketNumberRange::new(self.start, self.end)
    }

    /// Gets an iterator over the sent packet entries, sorted by PacketNumber
    #[inline]
    pub fn iter(&self) -> Iter<V> {
        Iter::new(self)
    }

    /// Returns true if there are no entries
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.index == self.values.len()
    }

    /// Clears all of the packet information in the sent
    #[inline]
    pub fn clear(&mut self) {
        self.index = self.values.len();
    }

    #[inline]
    fn pn_index(&self, packet_number: PacketNumber) -> Option<usize> {
        // the map is empty so there are no valid entries
        if self.is_empty() {
            return None;
        }

        // make sure it's within the inserted packet numbers
        if packet_number > self.end {
            return None;
        }

        let offset = packet_number.checked_distance(self.start)?;
        let index = self.index.checked_add(offset as usize)?;
        let index = index % self.values.len();
        Some(index)
    }

    #[inline]
    fn set_start(&mut self, packet_number: PacketNumber) {
        // this function assumes we have at least one element
        debug_assert!(!self.is_empty());
        debug_assert!(packet_number >= self.start);
        debug_assert!(packet_number <= self.end);

        // find the next occupied slot
        for packet_number in PacketNumberRange::new(packet_number, self.end) {
            if self.get(packet_number).is_some() {
                let index = self
                    .pn_index(packet_number)
                    .expect("packet should be in bounds");

                self.index = index;
                self.start = packet_number;
                debug_assert!(self.start <= self.end);
                debug_assert_eq!(self.pn_index(packet_number), Some(index));
                return;
            }
        }

        unreachable!("could not find an occupied entry; map should be empty");
    }

    #[inline]
    fn set_end(&mut self, packet_number: PacketNumber) {
        // this function assumes we have at least one element
        debug_assert!(!self.is_empty());
        debug_assert!(packet_number >= self.start);
        debug_assert!(packet_number <= self.end);

        // find the next occupied slot
        for packet_number in PacketNumberRange::new(self.start, packet_number).rev() {
            if self.get(packet_number).is_some() {
                self.end = packet_number;
                debug_assert!(self.start <= self.end);
                return;
            }
        }

        unreachable!("could not find an occupied entry; map should be empty");
    }

    fn resize(&mut self, len: usize) {
        let mut new_len = self.values.len();

        // grow capacity until we can fit the inserted PN
        loop {
            new_len *= 2;
            if len < new_len {
                break;
            }
        }

        // allocate a new packet buffer and copy the previous values
        let mut values = Vec::with_capacity(new_len);
        // The packets are stored in a ring so we copy from the index
        // to the end, then from the start to the index
        values.extend(self.values[self.index..].iter_mut().map(|v| v.take()));
        values.extend(self.values[..self.index].iter_mut().map(|v| v.take()));
        while values.len() != values.capacity() {
            values.push(None);
        }

        // reset the index to the beginning of the buffer
        self.index = 0;
        self.values = values.into_boxed_slice();
    }
}

/// An iterator over all of the contained packet numbers
///
/// This iterator is optimized to reduce the amount of bounds checks being performed
#[derive(Debug)]
pub struct Iter<'a, V> {
    packets: &'a Map<V>,
    packet_number: Option<PacketNumber>,
    index: usize,
    remaining: usize,
}

impl<'a, V> Iter<'a, V> {
    #[inline]
    fn new(packets: &'a Map<V>) -> Self {
        let start = packets.start;
        let end = packets.end;
        let index = packets.index;

        let mut iter = Self {
            packets,
            packet_number: Some(start),
            index,
            // start with an empty iterator
            remaining: 0,
        };

        // make sure we have at least one packet
        if iter.packets.is_empty() {
            return iter;
        }

        // set the number of remaining entries based on the bounded range
        iter.remaining = (end.as_u64() - start.as_u64()) as usize;
        // we always have at least 1 items since the range is inclusive
        iter.remaining += 1;

        debug_assert!(iter.remaining <= iter.packets.values.len());

        iter
    }
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (PacketNumber, &'a V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            self.remaining -= 1;

            let packet_number = self.packet_number?;
            self.packet_number = packet_number.next();

            let index = self.index;
            self.index = (index + 1) % self.packets.values.len();

            if let Some(info) = self.packets.values[index].as_ref() {
                return Some((packet_number, info));
            }
        }

        None
    }
}

/// An iterator which removes a set of packet numbers in a range
///
/// This iterator is optimized to reduce the amount of bounds checks being performed
#[derive(Debug)]
pub struct RemoveIter<'a, V> {
    packets: &'a mut Map<V>,
    packet_number: Option<PacketNumber>,
    index: usize,
    remaining: usize,
}

impl<'a, V> RemoveIter<'a, V> {
    #[inline]
    fn new(packets: &'a mut Map<V>, range: PacketNumberRange) -> Self {
        let mut start = packets.start;
        let mut end = packets.end;

        let index = packets.index;

        let mut iter = Self {
            packets,
            packet_number: None,
            index,
            // start with an empty iterator
            remaining: 0,
        };

        // make sure we have at least one packet
        if iter.packets.is_empty() {
            return iter;
        }

        // ensure the range overlaps with the contained items
        if range.end() < start || range.start() > end {
            return iter;
        }

        use core::cmp::Ordering::*;

        match (range.start().cmp(&start), range.end().cmp(&end)) {
            (Less, Equal) | (Less, Greater) | (Equal, Greater) | (Equal, Equal) => {
                // deleting all entries

                // clear the sent packets
                //
                // NOTE: this doesn't actually delete anything in the buffer
                iter.packets.clear();

                // no need to update index as it's already set to the lower bound
            }
            (Less, Less) | (Equal, Less) => {
                // deleting start
                end = range.end();

                iter.packets.set_start(end.next().unwrap());
            }
            (Greater, Greater) | (Greater, Equal) => {
                // deleting end
                start = range.start();

                iter.index = iter
                    .packets
                    .pn_index(start)
                    .expect("packet number bounds have already been checked");

                iter.packets.set_end(start.prev().unwrap());
            }
            (Greater, Less) => {
                // deleting middle part
                start = range.start();
                end = range.end();

                iter.index = iter
                    .packets
                    .pn_index(start)
                    .expect("packet number bounds have already been checked");
            }
        }

        // Update the starting packet number
        iter.packet_number = Some(start);
        // set the number of remaining entries based on the bounded range
        iter.remaining = (end.as_u64() - start.as_u64()) as usize;
        // we always have at least 1 items since the range is inclusive
        iter.remaining += 1;

        debug_assert!(iter.remaining <= iter.packets.values.len());

        iter
    }
}

impl<V> Iterator for RemoveIter<'_, V> {
    type Item = (PacketNumber, V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            self.remaining -= 1;

            let packet_number = self.packet_number?;
            self.packet_number = packet_number.next();

            let index = self.index;
            self.index = (index + 1) % self.packets.values.len();

            if let Some(info) = self.packets.values[index].take() {
                return Some((packet_number, info));
            }
        }

        None
    }
}

impl<V> Drop for RemoveIter<'_, V> {
    fn drop(&mut self) {
        // make sure the iterator is drained, otherwise the entries might dangle
        while self.next().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
        varint::VarInt,
    };
    use alloc::collections::BTreeMap;
    use bolero::{check, generator::*};

    type TestMap = Map<u64>;

    #[test]
    fn insert_get_range() {
        let mut sent_packets = TestMap::default();

        let packet_number_1 = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        let packet_number_2 = packet_number_1.next().unwrap();
        let packet_number_3 = packet_number_2.next().unwrap();

        sent_packets.insert(packet_number_1, 1);
        sent_packets.insert(packet_number_2, 2);

        assert!(sent_packets.get(packet_number_1).is_some());
        assert!(sent_packets.get(packet_number_2).is_some());
        assert!(sent_packets.get(packet_number_3).is_none());

        assert_eq!(sent_packets.get(packet_number_1).unwrap(), &1);
        assert_eq!(sent_packets.get(packet_number_2).unwrap(), &2);

        sent_packets.insert(packet_number_3, 3);

        assert!(sent_packets.get(packet_number_3).is_some());
        assert_eq!(sent_packets.get(packet_number_3).unwrap(), &3);

        for (packet_number, sent_packet_info) in sent_packets.iter() {
            assert_eq!(sent_packets.get(packet_number).unwrap(), sent_packet_info);
        }
    }

    #[test]
    fn remove() {
        let mut sent_packets = TestMap::default();
        let packet_number = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        sent_packets.insert(packet_number, 1);

        assert!(sent_packets.get(packet_number).is_some());
        assert_eq!(sent_packets.get(packet_number).unwrap(), &1);

        assert_eq!(Some(1), sent_packets.remove(packet_number));

        assert!(sent_packets.get(packet_number).is_none());

        // Removing a packet that was already removed doesn't panic
        assert_eq!(None, sent_packets.remove(packet_number));
    }

    #[test]
    fn empty() {
        let mut sent_packets = TestMap::default();
        assert!(sent_packets.is_empty());

        let packet_number = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        sent_packets.insert(packet_number, 1);
        assert!(!sent_packets.is_empty());
    }

    #[test]
    #[should_panic]
    fn wrong_packet_space_on_insert() {
        let mut sent_packets = new_sent_packets(PacketNumberSpace::Initial);

        let packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1));
        sent_packets.insert(packet_number, 1);
    }

    #[test]
    #[should_panic]
    fn wrong_packet_space_on_get() {
        let sent_packets = new_sent_packets(PacketNumberSpace::Initial);

        let packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1));
        sent_packets.get(packet_number);
    }

    #[test]
    #[should_panic]
    fn wrong_packet_space_on_remove_range() {
        let mut sent_packets = new_sent_packets(PacketNumberSpace::Initial);

        let packet_number_start =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1));
        let packet_number_end =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(2));
        sent_packets
            .remove_range(PacketNumberRange::new(
                packet_number_start,
                packet_number_end,
            ))
            .for_each(|_| ());
    }

    #[test]
    #[should_panic]
    fn wrong_packet_space_on_remove() {
        let mut sent_packets = new_sent_packets(PacketNumberSpace::Initial);

        let packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1));
        sent_packets.remove(packet_number);
    }

    fn new_sent_packets(space: PacketNumberSpace) -> TestMap {
        let mut sent_packets = TestMap::default();
        let packet_number = space.new_packet_number(VarInt::from_u8(0));
        sent_packets.insert(packet_number, 0);
        sent_packets
    }

    /// An operation to be performed against a model
    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Operation {
        // Inserts the current packet number
        Insert,
        // Skips the packet number
        Skip,
        // Removes a packet number
        Remove(VarInt),
        // Removes a range of packet numbers
        RemoveRange(VarInt, VarInt),
    }

    fn model(ops: &[Operation]) {
        let mut current = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0));

        /// Tracks the subject against an oracle to ensure differential equivalency
        #[derive(Debug, Default)]
        struct Model {
            subject: TestMap,
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
                // trim range so we're not slamming the BTreeMap
                let range = if self.subject.is_empty() {
                    PacketNumberRange::new(range.start(), range.start())
                } else {
                    let start = range.start().max(self.subject.start);
                    let end = range.end().min(self.subject.end);
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
                    let pn = PacketNumberSpace::ApplicationData.new_packet_number(pn);

                    model.remove(pn);
                }
                Operation::RemoveRange(start, end) => {
                    let (start, end) = if start > end {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    let start = PacketNumberSpace::ApplicationData.new_packet_number(start);
                    let end = PacketNumberSpace::ApplicationData.new_packet_number(end);
                    let range = PacketNumberRange::new(start, end);

                    model.remove_range(range);
                }
            }
        }
    }

    #[test]
    fn differential_test() {
        check!()
            .with_type::<Vec<Operation>>()
            .for_each(|ops| model(ops))
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(9), kani::solver(kissat))]
    fn insert_value() {
        // Confirm that a value is inserted
        check!().with_type().cloned().for_each(|pn| {
            let space = PacketNumberSpace::ApplicationData;
            let mut map = Map::default();
            assert!(map.is_empty());
            let pn = space.new_packet_number(pn);

            map.insert(pn, ());

            assert!(map.get(pn).is_some());
            assert!(!map.is_empty());
        });
    }
}
