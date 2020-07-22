use crate::{
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    varint::VarInt,
};
use alloc::collections::{btree_map::Range, BTreeMap};
use std::ops::RangeInclusive;

pub struct SentPackets {
    packet_space: PacketNumberSpace,
    sent_packets: BTreeMap<VarInt, SentPacket>,
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
    pub fn insert(&mut self, packet_number: PacketNumber, sent_packet: SentPacket) {
        assert_eq!(packet_number.space(), self.packet_space);
        self.sent_packets
            .insert(PacketNumber::as_varint(packet_number), sent_packet);
    }

    pub fn get(&self, packet_number: PacketNumber) -> Option<&SentPacket> {
        assert_eq!(packet_number.space(), self.packet_space);
        self.sent_packets
            .get(&PacketNumber::as_varint(packet_number))
    }

    pub fn range(&self, range: RangeInclusive<VarInt>) -> Range<'_, VarInt, SentPacket> {
        self.sent_packets.range(range)
    }
}

pub struct SentPacket {
    /// Indicates whether the packet counts towards bytes in flight
    pub in_flight: bool,
    /// The number of bytes sent in the packet, not including UDP or IP overhead,
    /// but including QUIC framing overhead
    pub sent_bytes: u64,
    /// The time the packet was sent
    pub time_sent: Timestamp,
}
