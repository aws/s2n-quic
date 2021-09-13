// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path;
use core::convert::TryInto;
use s2n_quic_core::{
    frame::ack_elicitation::AckElicitation, inet::ExplicitCongestionNotification,
    packet::number::Map as PacketNumberMap, time::Timestamp,
};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.1

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.1.1

pub type SentPackets = PacketNumberMap<SentPacketInfo>;

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
    /// The ECN marker (if any) sent on the datagram that contained this packet
    pub ecn: ExplicitCongestionNotification,
}

impl SentPacketInfo {
    pub fn new(
        congestion_controlled: bool,
        sent_bytes: usize,
        time_sent: Timestamp,
        ack_elicitation: AckElicitation,
        path_id: path::Id,
        ecn: ExplicitCongestionNotification,
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
            ecn,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{path, recovery::SentPacketInfo};
    use s2n_quic_core::{
        frame::ack_elicitation::AckElicitation, inet::ExplicitCongestionNotification,
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
            ExplicitCongestionNotification::default(),
        );
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
}
