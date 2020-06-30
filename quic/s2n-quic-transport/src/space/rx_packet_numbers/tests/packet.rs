use alloc::collections::VecDeque;
use core::ops::RangeInclusive;
use s2n_quic_core::{
    frame::{ack_elicitation::AckElicitation, Ack},
    inet::ExplicitCongestionNotification,
    packet::number::PacketNumber,
    time::Timestamp,
};

#[derive(Clone, Debug)]
pub struct Packet {
    pub packet_number: PacketNumber,
    pub ack_elicitation: AckElicitation,
    pub ecn: ExplicitCongestionNotification,
    pub time: Timestamp,
    pub ack: Option<Ack<VecDeque<RangeInclusive<PacketNumber>>>>,
}
