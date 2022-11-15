// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame::ack_elicitation::AckElicitation, inet::ExplicitCongestionNotification, path,
    time::Timestamp, transmission,
};
use core::convert::TryInto;

//= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.1

//= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.1.1

#[cfg(feature = "alloc")]
pub type SentPackets<PacketInfo> = crate::packet::number::Map<SentPacketInfo<PacketInfo>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SentPacketInfo<PacketInfo> {
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
    /// Indicates if the packet was part of a probe transmission
    pub transmission_mode: transmission::Mode,
    /// Additional packet metadata dictated by the congestion controller
    pub cc_packet_info: PacketInfo,
}

impl<PacketInfo> SentPacketInfo<PacketInfo> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        congestion_controlled: bool,
        sent_bytes: usize,
        time_sent: Timestamp,
        ack_elicitation: AckElicitation,
        path_id: path::Id,
        ecn: ExplicitCongestionNotification,
        transmission_mode: transmission::Mode,
        cc_packet_info: PacketInfo,
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
            transmission_mode,
            cc_packet_info,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        frame::ack_elicitation::AckElicitation,
        inet::ExplicitCongestionNotification,
        path,
        recovery::SentPacketInfo,
        time::{Clock, NoopClock},
        transmission,
    };

    #[test]
    #[should_panic]
    fn too_large_packet() {
        SentPacketInfo::new(
            true,
            u16::MAX as usize + 1,
            NoopClock.get_time(),
            AckElicitation::Eliciting,
            unsafe { path::Id::new(0) },
            ExplicitCongestionNotification::default(),
            transmission::Mode::Normal,
            (),
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
    fn sent_packet_info_size_test() {
        insta::assert_debug_snapshot!(
            stringify!(sent_packet_info_size_test),
            core::mem::size_of::<SentPacketInfo<()>>()
        );

        assert_eq!(
            core::mem::size_of::<Option<SentPacketInfo<()>>>(),
            core::mem::size_of::<SentPacketInfo<()>>()
        );
    }
}
