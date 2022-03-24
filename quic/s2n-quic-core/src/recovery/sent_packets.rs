// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame::ack_elicitation::AckElicitation, inet::ExplicitCongestionNotification, path,
    recovery::BandwidthEstimator, time::Timestamp,
};
use core::convert::TryInto;

//= https://www.rfc-editor.org/rfc/rfc9002#section-A.1

//= https://www.rfc-editor.org/rfc/rfc9002#section-A.1.1

#[cfg(feature = "alloc")]
pub type SentPackets = crate::packet::number::Map<SentPacketInfo>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// The amount of data in bytes delivered on the connection up to the point this packet was sent
    pub delivered_bytes: u64,
    /// The time a packet was last acknowledged at the point this packet was sent
    pub delivered_time: Timestamp,
    /// The amount of data in bytes lost on the connection up to the point this packet was sent
    pub lost_bytes: u64,
    /// The time of the first send in a flight of data
    pub first_sent_time: Timestamp,
    /// True if the connection was application-limited at the time this packet was sent
    pub is_app_limited: bool,
    /// The volume of data that was estimated to be in flight at the time of the transmission of this packet
    pub bytes_in_flight: u32,
}

impl SentPacketInfo {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        congestion_controlled: bool,
        sent_bytes: usize,
        time_sent: Timestamp,
        ack_elicitation: AckElicitation,
        path_id: path::Id,
        ecn: ExplicitCongestionNotification,
        bw_estimator: &BandwidthEstimator,
        bytes_in_flight: u32,
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
            delivered_bytes: bw_estimator.delivered_bytes(),
            delivered_time: bw_estimator
                .delivered_time()
                .expect("bw_estimator must be initialized when a packet is first sent"),
            lost_bytes: bw_estimator.lost_bytes(),
            first_sent_time: bw_estimator
                .first_sent_time()
                .expect("bw_estimator must be initialized when a packet is first sent"),
            is_app_limited: bw_estimator.app_limited_timestamp().is_some(),
            bytes_in_flight,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        frame::ack_elicitation::AckElicitation,
        inet::ExplicitCongestionNotification,
        path,
        recovery::{BandwidthEstimator, SentPacketInfo},
        time::{Clock, NoopClock},
    };

    #[test]
    #[should_panic]
    fn too_large_packet() {
        let mut bw_estimator = BandwidthEstimator::default();
        bw_estimator.on_packet_sent(false, false, NoopClock.get_time());

        SentPacketInfo::new(
            true,
            u16::MAX as usize + 1,
            NoopClock.get_time(),
            AckElicitation::Eliciting,
            unsafe { path::Id::new(0) },
            ExplicitCongestionNotification::default(),
            &bw_estimator,
            0,
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
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
