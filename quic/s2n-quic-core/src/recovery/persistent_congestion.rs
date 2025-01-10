// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::number::PacketNumber, path, recovery::SentPacketInfo, time::Timestamp};
use core::time::Duration;

#[derive(Debug)]
pub struct Calculator {
    current_period: Option<Period>,
    max_duration: Duration,
    first_rtt_sample: Option<Timestamp>,
    path_id: path::Id,
}

impl Calculator {
    /// Create a new `Calculator` for the given `path_id`
    #[inline]
    pub fn new(first_rtt_sample: Option<Timestamp>, path_id: path::Id) -> Self {
        Self {
            current_period: None,
            max_duration: Duration::ZERO,
            first_rtt_sample,
            path_id,
        }
    }

    /// Gets the longest persistent congestion period calculated
    #[inline]
    pub fn persistent_congestion_duration(&self) -> Duration {
        self.max_duration
    }

    /// Called for each packet detected as lost
    #[inline]
    pub fn on_lost_packet<PacketInfo>(
        &mut self,
        packet_number: PacketNumber,
        packet_info: &SentPacketInfo<PacketInfo>,
    ) {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
        //# The persistent congestion period SHOULD NOT start until there is at
        //# least one RTT sample.  Before the first RTT sample, a sender arms its
        //# PTO timer based on the initial RTT (Section 6.2.2), which could be
        //# substantially larger than the actual RTT.  Requiring a prior RTT
        //# sample prevents a sender from establishing persistent congestion with
        //# potentially too few probes.
        ensure!(self
            .first_rtt_sample
            .is_some_and(|ts| packet_info.time_sent >= ts));

        // Check that this lost packet was sent on the same path
        //
        // Persistent congestion is only updated for the path on which we receive
        // an ack. Managing state for multiple paths requires extra allocations
        // but is only necessary when also attempting connection_migration; which
        // should not be very common.
        ensure!(packet_info.path_id == self.path_id);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
        //# Loss of a QUIC packet that is carried in a PMTU probe is therefore not a
        //# reliable indication of congestion and SHOULD NOT trigger a congestion
        //# control reaction; see Item 7 in Section 3 of [DPLPMTUD].
        ensure!(!packet_info.transmission_mode.is_mtu_probing());

        if let Some(current_period) = &mut self.current_period {
            // We are currently tracking a persistent congestion period

            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
            //# A sender establishes persistent congestion after the receipt of an
            //# acknowledgment if two packets that are ack-eliciting are declared
            //# lost, and:
            //#
            //#     *  across all packet number spaces, none of the packets sent between
            //#        the send times of these two packets are acknowledged;

            // Check if this lost packet is contiguous with the current period.
            if current_period.is_contiguous(packet_number) {
                // Extend the end of the current persistent congestion period
                current_period.extend(packet_number, packet_info);

                self.max_duration = self.max_duration.max(current_period.duration());
            } else {
                // The current persistent congestion period has ended
                self.current_period = None
            }
        }

        if self.current_period.is_none() && packet_info.ack_elicitation.is_ack_eliciting() {
            // Start tracking a new persistent congestion period

            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
            //# These two packets MUST be ack-eliciting, since a receiver is required
            //# to acknowledge only ack-eliciting packets within its maximum
            //# acknowledgment delay; see Section 13.2 of [QUIC-TRANSPORT].
            self.current_period = Some(Period::new(packet_info.time_sent, packet_number));
        }
    }
}

#[derive(Debug)]
struct Period {
    start: Timestamp,
    end: Timestamp,
    prev_packet: PacketNumber,
}

impl Period {
    /// Creates a new `Period`
    #[inline]
    fn new(start: Timestamp, packet_number: PacketNumber) -> Self {
        Self {
            start,
            end: start,
            prev_packet: packet_number,
        }
    }

    /// True if the given packet number is 1 more than the last packet in this period
    #[inline]
    fn is_contiguous(&self, packet_number: PacketNumber) -> bool {
        packet_number.checked_distance(self.prev_packet) == Some(1)
    }

    /// Extends this persistent congestion period
    #[inline]
    fn extend<PacketInfo>(
        &mut self,
        packet_number: PacketNumber,
        packet_info: &SentPacketInfo<PacketInfo>,
    ) {
        debug_assert!(self.is_contiguous(packet_number));
        debug_assert!(packet_info.time_sent >= self.start);

        if packet_info.ack_elicitation.is_ack_eliciting() {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
            //# These two packets MUST be ack-eliciting, since a receiver is required
            //# to acknowledge only ack-eliciting packets within its maximum
            //# acknowledgment delay; see Section 13.2 of [QUIC-TRANSPORT].
            self.end = packet_info.time_sent;
        }

        self.prev_packet = packet_number;
    }

    /// Gets the duration of this persistent congestion period
    #[inline]
    fn duration(&self) -> Duration {
        self.end - self.start
    }
}
