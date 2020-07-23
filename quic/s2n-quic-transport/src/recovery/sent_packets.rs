use alloc::collections::{btree_map::Range, BTreeMap};
use s2n_quic_core::{
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
};
use std::ops::RangeInclusive;

#[derive(Clone, Debug)]
pub struct SentPackets {
    packet_space: PacketNumberSpace,
    sent_packets: BTreeMap<PacketNumber, SentPacketInfo>,
}

impl SentPackets {
    pub fn new(packet_space: PacketNumberSpace) -> Self {
        Self {
            packet_space,
            sent_packets: BTreeMap::default(),
        }
    }
}

impl SentPackets {
    pub fn insert(&mut self, packet_number: PacketNumber, sent_packet: SentPacketInfo) {
        assert_eq!(packet_number.space(), self.packet_space);
        self.sent_packets.insert(packet_number, sent_packet);
    }

    pub fn get(&self, packet_number: PacketNumber) -> Option<&SentPacketInfo> {
        assert_eq!(packet_number.space(), self.packet_space);
        self.sent_packets.get(&packet_number)
    }

    pub fn range(
        &self,
        range: RangeInclusive<PacketNumber>,
    ) -> Range<'_, PacketNumber, SentPacketInfo> {
        self.sent_packets.range(range)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SentPacketInfo {
    /// Indicates whether the packet counts towards bytes in flight
    pub in_flight: bool,
    /// The number of bytes sent in the packet, not including UDP or IP overhead,
    /// but including QUIC framing overhead
    pub sent_bytes: u64,
    /// The time the packet was sent
    pub time_sent: Timestamp,
}
