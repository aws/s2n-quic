use s2n_quic_core::{inet::ExplicitCongestionNotification, time::Timestamp};

pub type Map<Data> = s2n_quic_core::packet::number::Map<SentPacketInfo<Data>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SentPacketInfo<Data> {
    pub data: Data,
    pub time_sent: Timestamp,
    pub ecn: ExplicitCongestionNotification,
    pub cc_info: crate::congestion::PacketInfo,
}
