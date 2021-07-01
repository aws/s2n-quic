// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path;
use core::convert::TryInto;
use s2n_quic_core::{
    frame::ack_elicitation::AckElicitation,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    time::Timestamp,
};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.1

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.1.1

/// A data structure for tracking sent packets that are pending acknowledgement
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
///                              ^ index = 3
/// start = PN(1)
/// end = PN(3)
/// ```
#[derive(Clone, Debug)]
pub struct SentPackets {
    /// The sent packet info buffer
    packets: Box<[Option<SentPacketInfo>]>,
    /// The smallest contained inclusive packet number in the set
    start: PacketNumber,
    /// The largest contained inclusive packet number in the set
    end: PacketNumber,
    /// The starting index of the first occupied packet
    ///
    /// This field will be set to the `packets.len()` if the set is empty
    index: usize,
}

/// Start with 8 sent packets at a time
///
/// Capacity will grow exponentially as more concurrent packets are being sent.
const DEFAULT_CAPACITY: usize = 8;

impl Default for SentPackets {
    fn default() -> Self {
        // we use the Initial packet number space as a filler until an actual
        // packet number is inserted
        let base = PacketNumberSpace::Initial.new_packet_number(0u8.into());

        let packets = vec![None; DEFAULT_CAPACITY].into_boxed_slice();

        // Set the index to the len (OOB) to indicate that it's empty
        let index = packets.len();

        Self {
            packets,
            start: base,
            end: base,
            index,
        }
    }
}

impl SentPackets {
    /// Inserts the given `sent_packet_info`
    pub fn insert(&mut self, packet_number: PacketNumber, sent_packet_info: SentPacketInfo) {
        if self.is_empty() {
            self.start = packet_number;
            self.end = packet_number;
            self.packets[0] = Some(sent_packet_info);
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
        let distance = packet_number.as_u64() - self.start.as_u64();

        let index = if distance >= self.packets.len() as u64 {
            let mut new_len = self.packets.len();

            // grow capacity until we can fit the inserted PN
            loop {
                new_len *= 2;
                if distance < (new_len as u64) {
                    break;
                }
            }

            // allocate a new packet buffer and copy the previous values
            let mut packets = Vec::with_capacity(new_len);
            // The packets are stored in a ring so we copy from the index
            // to the end, then from the start to the index
            packets.extend_from_slice(&self.packets[self.index..]);
            packets.extend_from_slice(&self.packets[..self.index]);
            packets.resize(new_len, None);

            // reset the index to the beginning of the buffer
            self.index = 0;
            self.packets = packets.into_boxed_slice();

            // use the distance as the index
            distance as usize
        } else {
            // we can't use pn_index_get here since it will bail due to `packet_number > self.end`
            let mut index = self.pn_index_unbound(packet_number).unwrap();
            index %= self.packets.len();
            index
        };

        self.packets[index] = Some(sent_packet_info);
        self.end = packet_number;
    }

    /// Returns a reference to the `SentPacketInfo` associated with the given `packet_number`
    #[inline]
    pub fn get(&self, packet_number: PacketNumber) -> Option<&SentPacketInfo> {
        let index = self.pn_index_get(packet_number)?;
        self.packets[index].as_ref()
    }

    /// Removes the `SentPacketInfo` associated with the given `packet_number`
    /// and returns the `SentPacketInfo` if it was present
    pub fn remove(&mut self, packet_number: PacketNumber) -> Option<SentPacketInfo> {
        let index = self.pn_index_get(packet_number)?;
        let info = self.packets[index].take()?;

        // update the bounds
        match (self.start == packet_number, self.end == packet_number) {
            // the bounds are inclusive so the set is now empty, reset it
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

    /// Removes a range of packets from the set and returns their information if present
    #[inline]
    pub fn remove_range(&mut self, range: PacketNumberRange) -> RemoveIter {
        RemoveIter::new(self, range)
    }

    /// Gets an iterator over the sent packet entries, sorted by PacketNumber
    #[inline]
    pub fn iter(&self) -> Iter {
        Iter::new(self)
    }

    /// Returns true if there are no pending sent packets
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.index == self.packets.len()
    }

    /// Clears all of the packet information in the sent
    #[inline]
    pub fn clear(&mut self) {
        self.index = self.packets.len();
    }

    #[inline]
    fn pn_index_unbound(&self, packet_number: PacketNumber) -> Option<usize> {
        // the set is empty so there are no valid entries
        if self.is_empty() {
            return None;
        }

        let offset = packet_number.checked_distance(self.start)?;

        let index = self.index.checked_add(offset as usize)?;
        Some(index)
    }

    #[inline]
    fn pn_index_get(&self, packet_number: PacketNumber) -> Option<usize> {
        let index = self.pn_index_unbound(packet_number)?;

        // make sure it's within the inserted packet numbers
        if packet_number > self.end {
            return None;
        }

        let index = index % self.packets.len();
        Some(index)
    }

    #[inline]
    fn pn_range(&self) -> PacketNumberRange {
        PacketNumberRange::new(self.start, self.end)
    }

    fn set_start(&mut self, packet_number: PacketNumber) {
        // this function assumes we have at least one element
        debug_assert!(!self.is_empty());
        debug_assert!(packet_number >= self.start);
        debug_assert!(packet_number <= self.end);

        // find the next occupied slot
        for packet_number in PacketNumberRange::new(packet_number, self.end) {
            if self.get(packet_number).is_some() {
                let index = self
                    .pn_index_get(packet_number)
                    .expect("packet should be in bounds");

                self.index = index;
                self.start = packet_number;
                debug_assert!(self.start <= self.end);
                debug_assert_eq!(self.pn_index_get(packet_number), Some(index));
                return;
            }
        }

        unreachable!("could not find an occupied entry; set should be empty");
    }

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

        unreachable!("could not find an occupied entry; set should be empty");
    }
}

/// An iterator over all of the contained packet numbers
///
/// This iterator is optimized to reduce the amount of bounds checks being performed
#[derive(Debug)]
pub struct Iter<'a> {
    sent_packets: &'a SentPackets,
    packet_number: Option<PacketNumber>,
    index: usize,
    remaining: usize,
}

impl<'a> Iter<'a> {
    fn new(sent_packets: &'a SentPackets) -> Self {
        let start = sent_packets.start;
        let end = sent_packets.end;
        let index = sent_packets.index;

        let mut iter = Self {
            sent_packets,
            packet_number: Some(start),
            index,
            // start with an empty iterator
            remaining: 0,
        };

        // make sure we have at least one packet
        if iter.sent_packets.is_empty() {
            return iter;
        }

        // set the number of remaining entries based on the bounded range
        iter.remaining = (end.as_u64() - start.as_u64()) as usize;
        // we always have at least 1 items since the range is inclusive
        iter.remaining += 1;

        debug_assert!(iter.remaining <= iter.sent_packets.packets.len());

        iter
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (PacketNumber, &'a SentPacketInfo);

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            self.remaining -= 1;

            let packet_number = self.packet_number?;
            self.packet_number = packet_number.next();

            let index = self.index;
            self.index = (index + 1) % self.sent_packets.packets.len();

            if let Some(info) = self.sent_packets.packets[index].as_ref() {
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
pub struct RemoveIter<'a> {
    sent_packets: &'a mut SentPackets,
    packet_number: Option<PacketNumber>,
    index: usize,
    remaining: usize,
}

impl<'a> RemoveIter<'a> {
    fn new(sent_packets: &'a mut SentPackets, range: PacketNumberRange) -> Self {
        let mut start = sent_packets.start;
        let mut end = sent_packets.end;

        let prev_range = sent_packets.pn_range();

        let index = sent_packets.index;

        let mut iter = Self {
            sent_packets,
            packet_number: None,
            index,
            // start with an empty iterator
            remaining: 0,
        };

        // make sure we have at least one packet
        if iter.sent_packets.is_empty() {
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
                iter.sent_packets.clear();

                // no need to update index as it's already set to the lower bound
            }
            (Less, Less) | (Equal, Less) => {
                // deleting start
                end = range.end();

                iter.sent_packets.set_start(end.next().unwrap());
            }
            (Greater, Greater) | (Greater, Equal) => {
                // deleting end
                start = range.start();

                iter.index = iter
                    .sent_packets
                    .pn_index_get(start)
                    .expect("packet number bounds have already been checked");

                iter.sent_packets.set_end(start.prev().unwrap());
            }
            (Greater, Less) => {
                // deleting middle part
                start = range.start();
                end = range.end();

                iter.index = iter
                    .sent_packets
                    .pn_index_get(start)
                    .expect("packet number bounds have already been checked");
            }
        }

        // Update the starting packet number
        iter.packet_number = Some(start);
        // set the number of remaining entries based on the bounded range
        iter.remaining = (end.as_u64() - start.as_u64()) as usize;
        // we always have at least 1 items since the range is inclusive
        iter.remaining += 1;

        debug_assert!(prev_range.start() <= start);
        debug_assert!(prev_range.end() >= end);
        debug_assert!(iter.remaining <= iter.sent_packets.packets.len());

        iter
    }
}

impl<'a> Iterator for RemoveIter<'a> {
    type Item = (PacketNumber, SentPacketInfo);

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            self.remaining -= 1;

            let packet_number = self.packet_number?;
            self.packet_number = packet_number.next();

            let index = self.index;
            self.index = (index + 1) % self.sent_packets.packets.len();

            if let Some(info) = self.sent_packets.packets[index].take() {
                return Some((packet_number, info));
            }
        }

        None
    }
}

#[cfg(debug_assertions)]
impl<'a> Drop for RemoveIter<'a> {
    fn drop(&mut self) {
        assert!(
            self.remaining == 0,
            "dropping a remove iterator before draining it will leave occupied packets"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SentPacketInfo {
    /// Indicates whether the packet counts towards bytes in flight
    pub congestion_controlled: bool,
    /// The number of bytes sent in the packet, not including UDP or IP overhead,
    /// but including QUIC framing overhead
    pub sent_bytes: u16,
    /// The time the packet was sent
    pub time_sent: Timestamp,
    /// Indicates whether a packet is ack-eliciting
    pub ack_elicitation: AckElicitation,
    /// The ID of the Path the packet was sent on
    pub path_id: path::Id,
}

impl SentPacketInfo {
    pub fn new(
        congestion_controlled: bool,
        sent_bytes: usize,
        time_sent: Timestamp,
        ack_elicitation: AckElicitation,
        path_id: path::Id,
    ) -> Self {
        debug_assert_eq!(
            sent_bytes > 0,
            congestion_controlled,
            "sent bytes should be zero for packets that are not congestion controlled"
        );

        SentPacketInfo {
            congestion_controlled,
            sent_bytes: sent_bytes
                .try_into()
                .expect("sent_bytes exceeds max UDP payload size"),
            time_sent,
            ack_elicitation,
            path_id,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        interval_set::IntervalSet,
        path,
        recovery::{SentPacketInfo, SentPackets},
    };
    use bolero::{check, generator::*};
    use s2n_quic_core::{
        frame::ack_elicitation::AckElicitation,
        packet::number::{PacketNumberRange, PacketNumberSpace},
        varint::VarInt,
    };

    #[test]
    #[should_panic]
    fn too_large_packet() {
        SentPacketInfo::new(
            true,
            u16::max_value() as usize + 1,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );
    }

    #[test]
    fn insert_get_range() {
        let mut sent_packets = SentPackets::default();

        let packet_number_1 = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        let sent_packet_1 = SentPacketInfo::new(
            true,
            1,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );

        let packet_number_2 = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(2));
        let sent_packet_2 = SentPacketInfo::new(
            true,
            2,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );

        let packet_number_3 = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(3));
        let sent_packet_3 = SentPacketInfo::new(
            true,
            3,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );

        sent_packets.insert(packet_number_1, sent_packet_1);
        sent_packets.insert(packet_number_2, sent_packet_2);

        assert!(sent_packets.get(packet_number_1).is_some());
        assert!(sent_packets.get(packet_number_2).is_some());
        assert!(sent_packets.get(packet_number_3).is_none());

        assert_eq!(sent_packets.get(packet_number_1).unwrap(), &sent_packet_1);
        assert_eq!(sent_packets.get(packet_number_2).unwrap(), &sent_packet_2);

        sent_packets.insert(packet_number_3, sent_packet_3);

        assert!(sent_packets.get(packet_number_3).is_some());
        assert_eq!(sent_packets.get(packet_number_3).unwrap(), &sent_packet_3);

        for (packet_number, sent_packet_info) in sent_packets.iter() {
            assert_eq!(sent_packets.get(packet_number).unwrap(), sent_packet_info);
        }
    }

    #[test]
    fn remove() {
        let mut sent_packets = SentPackets::default();
        let packet_number = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        let sent_packet = SentPacketInfo::new(
            false,
            0,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );
        sent_packets.insert(packet_number, sent_packet);

        assert!(sent_packets.get(packet_number).is_some());
        assert_eq!(sent_packets.get(packet_number).unwrap(), &sent_packet);

        assert_eq!(Some(sent_packet), sent_packets.remove(packet_number));

        assert!(sent_packets.get(packet_number).is_none());

        // Removing a packet that was already removed doesn't panic
        assert_eq!(None, sent_packets.remove(packet_number));
    }

    #[test]
    fn empty() {
        let mut sent_packets = SentPackets::default();
        assert!(sent_packets.is_empty());

        let packet_number = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(1));
        let sent_packet = SentPacketInfo::new(
            false,
            0,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );
        sent_packets.insert(packet_number, sent_packet);
        assert!(!sent_packets.is_empty());
    }

    #[test]
    #[should_panic]
    fn wrong_packet_space_on_insert() {
        let mut sent_packets = new_sent_packets(PacketNumberSpace::Initial);

        let packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1));
        let sent_packet = SentPacketInfo::new(
            false,
            0,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );

        sent_packets.insert(packet_number, sent_packet);
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

    fn new_sent_packets(space: PacketNumberSpace) -> SentPackets {
        let mut sent_packets = SentPackets::default();
        let packet_number = space.new_packet_number(VarInt::from_u8(1));
        let sent_packet = SentPacketInfo::new(
            false,
            0,
            s2n_quic_platform::time::now(),
            AckElicitation::Eliciting,
            path::Id::new(0),
        );
        sent_packets.insert(packet_number, sent_packet);
        sent_packets
    }

    #[test]
    fn sent_packet_info_size_test() {
        insta::assert_debug_snapshot!(
            stringify!(sent_packet_info_size_test),
            core::mem::size_of::<SentPacketInfo>()
        );

        assert_eq!(
            core::mem::size_of::<Option<SentPacketInfo>>(),
            core::mem::size_of::<SentPacketInfo>()
        );
    }

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
        use s2n_quic_core::packet::number::PacketNumber;

        let mut current = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0));

        #[derive(Debug, Default)]
        struct Model {
            subject: SentPackets,
            oracle: std::collections::BTreeMap<PacketNumber, SentPacketInfo>,
            active: IntervalSet<PacketNumber>,
        }

        impl Model {
            pub fn insert(&mut self, packet_number: PacketNumber) {
                let sent_bytes = packet_number.as_u64() as u16 as usize;
                let info = SentPacketInfo::new(
                    sent_bytes != 0,
                    sent_bytes,
                    s2n_quic_platform::time::now(),
                    AckElicitation::Eliciting,
                    path::Id::new(0),
                );

                self.subject.insert(packet_number, info);
                self.oracle.insert(packet_number, info);
                self.active.insert_value(packet_number).unwrap();
                self.check_consistency();
            }

            pub fn remove(&mut self, packet_number: PacketNumber) {
                assert_eq!(
                    self.subject.remove(packet_number),
                    self.oracle.remove(&packet_number)
                );
                self.active.remove_value(packet_number).unwrap();
                self.check_consistency();
            }

            pub fn remove_range(&mut self, range: PacketNumberRange) {
                let actual: Vec<_> = self.subject.remove_range(range).collect();
                let mut expected = vec![];

                let mut trimmed = IntervalSet::with_capacity(1);
                trimmed.insert(range.start()..=range.end()).unwrap();

                // trim the range down so we're not slamming the BTreeMap
                for range in trimmed.intersection_iter(&self.active) {
                    for pn in range {
                        if let Some(info) = self.oracle.remove(&pn) {
                            expected.push((pn, info));
                        }
                    }
                }

                self.active.remove(range.start()..=range.end()).unwrap();

                assert_eq!(expected, actual);

                self.check_consistency();
            }

            fn check_consistency(&self) {
                if let Some(expected_start) = self.active.min_value() {
                    assert_eq!(self.subject.start, expected_start);
                } else {
                    assert!(self.subject.is_empty());
                }

                if let Some(expected_end) = self.active.max_value() {
                    assert_eq!(self.subject.end, expected_end);
                }

                let mut subject = self.subject.iter();
                let mut oracle = self.oracle.iter();
                loop {
                    match (subject.next(), oracle.next()) {
                        (Some(actual), Some((expected_pn, expected_info))) => {
                            assert_eq!((*expected_pn, expected_info), actual);
                        }
                        (None, None) => break,
                        (actual, expected) => {
                            panic!("expected: {:?}, actual: {:?}", expected, actual);
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
}
