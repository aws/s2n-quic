// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::{PacketNumber, PacketNumberRange},
    recovery::SentPacketInfo,
    time::Timestamp,
};
use core::time::Duration;

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.1
//# The RECOMMENDED initial value for the packet reordering threshold
//# (kPacketThreshold) is 3, based on best practices for TCP loss
//# detection [RFC5681] [RFC6675].  In order to remain similar to TCP,
//# implementations SHOULD NOT use a packet threshold less than 3; see
//# [RFC5681].
const K_PACKET_THRESHOLD: u64 = 3;

pub enum Outcome {
    NotLost { lost_time: Timestamp },
    Lost,
}

#[derive(Debug, Default)]
pub struct Detector {}

impl Detector {
    pub fn check_iter<'a, P: 'a>(
        sent_packets: impl Iterator<Item = &'a SentPacketInfo<P>>,
    ) -> Option<PacketNumberRange> {
        None
    }

    pub fn check(
        &self,
        time_threshold: Duration,
        time_sent: Timestamp,
        packet_number: PacketNumber,
        largest_acked_packet_number: PacketNumber,
        now: Timestamp,
    ) -> Outcome {
        // Calculate at what time this particular packet is considered lost based on the
        // current path `time_threshold`
        let packet_lost_time = time_sent + time_threshold;

        // If the `packet_lost_time` exceeds the current time, it's lost
        let time_threshold_exceeded = packet_lost_time.has_elapsed(now);

        let packet_number_threshold_exceeded = largest_acked_packet_number
            .checked_distance(packet_number)
            .expect("largest_acked_packet_number >= packet_number")
            >= K_PACKET_THRESHOLD;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1
        //# A packet is declared lost if it meets all of the following
        //# conditions:
        //#
        //#     *  The packet is unacknowledged, in flight, and was sent prior to an
        //#        acknowledged packet.
        //#
        //#     *  The packet was sent kPacketThreshold packets before an
        //#        acknowledged packet (Section 6.1.1), or it was sent long enough in
        //#        the past (Section 6.1.2).
        if time_threshold_exceeded || packet_number_threshold_exceeded {
            return Outcome::Lost;
        }

        Outcome::NotLost {
            lost_time: packet_lost_time,
        }
    }
}
