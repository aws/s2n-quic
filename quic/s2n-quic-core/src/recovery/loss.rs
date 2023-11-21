// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::number::PacketNumber, time::Timestamp};
use core::time::Duration;

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.1
//# The RECOMMENDED initial value for the packet reordering threshold
//# (kPacketThreshold) is 3, based on best practices for TCP loss
//# detection [RFC5681] [RFC6675].  In order to remain similar to TCP,
//# implementations SHOULD NOT use a packet threshold less than 3; see
//# [RFC5681].
pub const K_PACKET_THRESHOLD: u64 = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The packet is not lost yet, but will be considered lost at the
    /// given `lost_time` if not acknowledged by then
    NotLostYet { lost_time: Timestamp },
    /// The packet is lost
    Lost,
}

/// Detect if the given packet number is lost based on how long ago
/// it was sent and how far from the largest acked packet number it is.
pub fn detect(
    time_threshold: Duration,
    time_sent: Timestamp,
    packet_number: PacketNumber,
    largest_acked_packet_number: PacketNumber,
    now: Timestamp,
) -> Outcome {
    debug_assert!(largest_acked_packet_number >= packet_number);

    // Calculate at what time this particular packet is considered
    // lost based on the `time_threshold`
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

    Outcome::NotLostYet {
        lost_time: packet_lost_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{packet::number::PacketNumberSpace, time::testing::now};

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.1
    //= type=test
    //# The RECOMMENDED initial value for the packet reordering threshold
    //# (kPacketThreshold) is 3, based on best practices for TCP loss
    //# detection [RFC5681] [RFC6675].

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.1
    //= type=test
    //# In order to remain similar to TCP,
    //# implementations SHOULD NOT use a packet threshold less than 3; see
    //# [RFC5681].
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn packet_reorder_threshold_at_least_three() {
        assert!(K_PACKET_THRESHOLD >= 3);
    }

    #[test]
    fn time_threshold() {
        let time_threshold = Duration::from_secs(5);
        let packet_number = new_packet_number(1);
        // largest acked is within the K_PACKET_THRESHOLD
        let largest_acked_packet_number =
            new_packet_number(packet_number.as_u64() + K_PACKET_THRESHOLD - 1);

        let time_sent = now();
        let current_time = time_sent + time_threshold;

        let outcome = detect(
            time_threshold,
            time_sent,
            packet_number,
            largest_acked_packet_number,
            current_time,
        );

        assert_eq!(Outcome::Lost, outcome);

        let time_sent = now();
        let current_time = time_sent + time_threshold - Duration::from_secs(1);

        let outcome = detect(
            time_threshold,
            time_sent,
            packet_number,
            largest_acked_packet_number,
            current_time,
        );

        assert_eq!(
            Outcome::NotLostYet {
                lost_time: current_time + Duration::from_secs(1)
            },
            outcome
        );
    }

    #[test]
    fn packet_number_threshold() {
        let time_threshold = Duration::from_secs(5);
        let time_sent = now();
        // packet was sent less than the time threshold in the past
        let current_time = time_sent + time_threshold - Duration::from_secs(1);

        let packet_number = new_packet_number(1);
        // largest acked is K_PACKET_THRESHOLD larger than the current packet
        let largest_acked_packet_number =
            new_packet_number(packet_number.as_u64() + K_PACKET_THRESHOLD);

        let outcome = detect(
            time_threshold,
            time_sent,
            packet_number,
            largest_acked_packet_number,
            current_time,
        );

        assert_eq!(Outcome::Lost, outcome);

        // largest acked is within the K_PACKET_THRESHOLD
        let largest_acked_packet_number =
            new_packet_number(packet_number.as_u64() + K_PACKET_THRESHOLD - 1);

        let outcome = detect(
            time_threshold,
            time_sent,
            packet_number,
            largest_acked_packet_number,
            current_time,
        );

        assert_eq!(
            Outcome::NotLostYet {
                lost_time: current_time + Duration::from_secs(1)
            },
            outcome
        );
    }

    fn new_packet_number(packet_number: u64) -> PacketNumber {
        PacketNumberSpace::ApplicationData.new_packet_number(packet_number.try_into().unwrap())
    }
}
