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
#[derive(Clone, Debug)]
pub struct SentPackets {
    packets: Vec<Option<SentPacketInfo>>,
    // The smallest contained packet number in the set
    start: PacketNumber,
    // The largest contained packet number in the set
    end: PacketNumber,
    // The starting index of the first occupied packet
    index: usize,
}

/// Start with 8 sent packets at a time
///
/// Capacity will grow exponentially as more concurrent packets are being sent.
const DEFAULT_CAPACITY: usize = 8;

impl Default for SentPackets {
    fn default() -> Self {
        let base = PacketNumberSpace::Initial.new_packet_number(0u8.into());
        let packets = vec![None; DEFAULT_CAPACITY];
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
            unsafe {
                // Safety: packets should always have an element
                *self.packets.get_unchecked_mut(0) = Some(sent_packet_info);
            }
            self.index = 0;
            return;
        }

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
            let mut packets = Vec::with_capacity(new_len);
            packets.extend_from_slice(&self.packets[self.index..]);
            packets.extend_from_slice(&self.packets[..self.index]);
            packets.resize(new_len, None);
            self.index = 0;
            self.packets = packets;

            self.pn_index(packet_number).unwrap()
        } else {
            let mut index = self.pn_index(packet_number).unwrap();
            index %= self.packets.len();
            index
        };

        unsafe {
            debug_assert!(index < self.packets.len());
            *self.packets.get_unchecked_mut(index) = Some(sent_packet_info);
            self.end = self.end.max(packet_number);
        }
    }

    /// Returns a reference to the `SentPacketInfo` associated with the given `packet_number`
    #[inline]
    pub fn get(&self, packet_number: PacketNumber) -> Option<&SentPacketInfo> {
        let index = self.pn_index_get(packet_number)?;
        unsafe {
            // Safety: index is wrapped around packets.len
            debug_assert!(index < self.packets.len());
            self.packets.get_unchecked(index).as_ref()
        }
    }

    /// Removes the `SentPacketInfo` associated with the given `packet_number`
    /// and returns the `SentPacketInfo` if it was present
    pub fn remove(&mut self, packet_number: PacketNumber) -> Option<SentPacketInfo> {
        let index = self.pn_index_get(packet_number)?;

        let info = unsafe {
            // Safety: index is wrapped around packets.len
            debug_assert!(index < self.packets.len());
            self.packets.get_unchecked_mut(index).take()?
        };

        match (self.start == packet_number, self.end == packet_number) {
            // the set is now empty, reset it
            (true, true) => {
                self.index = self.packets.len();
            }
            // the packet was removed from the front
            (true, false) => {
                let (start, _) = self.iter().next().expect("set should not be empty");
                self.index = index + (start.as_u64() - self.start.as_u64()) as usize;
                self.index %= self.packets.len();
                self.start = start;
            }
            (false, true) => {
                let (end, _) = self.iter().next_back().expect("set should not be empty");
                self.end = end;
            }
            (false, false) => {
                // removing from the middle - do nothing
            }
        }

        Some(info)
    }

    pub fn remove_range(
        &mut self,
        range: PacketNumberRange,
    ) -> impl Iterator<Item = (PacketNumber, SentPacketInfo)> + '_ {
        let start = range.start().max(self.start);
        let end = range.end().min(self.end);
        let range = if self.is_empty() || start > end {
            None
        } else {
            Some(PacketNumberRange::new(start, end))
        };

        // TODO optimize this into a specialized iterator
        range
            .into_iter()
            .flatten()
            .filter_map(move |packet_number| {
                let info = self.remove(packet_number)?;
                Some((packet_number, info))
            })
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

    #[inline]
    fn pn_index(&self, packet_number: PacketNumber) -> Option<usize> {
        let index = self.index;
        // the set is empty
        if index == self.packets.len() {
            return None;
        }

        let offset = packet_number.checked_distance(self.start)?;

        let index = index.checked_add(offset as usize)?;
        Some(index)
    }

    #[inline]
    fn pn_index_get(&self, packet_number: PacketNumber) -> Option<usize> {
        let index = self.pn_index(packet_number)?;

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
}

pub struct Iter<'a> {
    sent_packets: &'a SentPackets,
    range: PacketNumberRange,
}

impl<'a> Iter<'a> {
    fn new(sent_packets: &'a SentPackets) -> Self {
        let mut range = sent_packets.pn_range();
        // exhaust the range to make it empty
        if sent_packets.is_empty() {
            let _ = range.next();
        }
        Self {
            sent_packets,
            range,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (PacketNumber, &'a SentPacketInfo);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let pn = self.range.next()?;
            if let Some(info) = self.sent_packets.get(pn) {
                return Some((pn, info));
            }
        }
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let pn = self.range.next_back()?;
            if let Some(info) = self.sent_packets.get(pn) {
                return Some((pn, info));
            }
        }
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
        path,
        recovery::{SentPacketInfo, SentPackets},
    };
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

    use crate::interval_set::IntervalSet;
    use bolero::{check, generator::*};

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

        let mut current = PacketNumberSpace::Initial.new_packet_number(VarInt::from_u8(0));

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
                    let pn = PacketNumberSpace::Initial.new_packet_number(pn);

                    model.remove(pn);
                }
                Operation::RemoveRange(start, end) => {
                    let (start, end) = if start > end {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    let start = PacketNumberSpace::Initial.new_packet_number(start);
                    let end = PacketNumberSpace::Initial.new_packet_number(end);
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
