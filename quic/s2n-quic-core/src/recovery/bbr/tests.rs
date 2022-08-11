// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::MINIMUM_MTU,
    recovery::{bandwidth::PacketInfo, bbr::BbrCongestionController},
    time::{Clock, NoopClock},
};

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
//= type=test
//# BBRInflightHiFromLostPacket(rs, packet):
//#   size = packet.size
//#   /* What was in flight before this packet? */
//#   inflight_prev = rs.tx_in_flight - size
//#   /* What was lost before this packet? */
//#   lost_prev = rs.lost - size
//#   lost_prefix = (BBRLossThresh * inflight_prev - lost_prev) /
//#                 (1 - BBRLossThresh)
//#   /* At what inflight value did losses cross BBRLossThresh? */
//#   inflight = inflight_prev + lost_prefix
//#   return inflight
#[test]
fn inflight_hi_from_lost_packet() {
    let now = NoopClock.get_time();
    let packet_info = PacketInfo {
        delivered_bytes: 0,
        delivered_time: now,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time: now,
        bytes_in_flight: 3000,
        is_app_limited: false,
    };

    let inflight_prev = packet_info.bytes_in_flight - MINIMUM_MTU as u32;
    assert_eq!(inflight_prev, 1800);

    // lost prefix = (BBRLossThresh * inflight_prev - lost_prev) / (1 - BBRLossThresh)
    // lost prefix = (1/50 * 1800 - (1210 - 1200)) / (1 - 1/50) = ~26
    assert_eq!(
        inflight_prev + 26,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MTU as u32,
            1210,
            packet_info
        )
    );

    // lost prefix is zero since LOSS_THRESH * 1800 < 3000 - 1200
    assert_eq!(
        inflight_prev,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MTU as u32,
            3000,
            packet_info
        )
    );
}
