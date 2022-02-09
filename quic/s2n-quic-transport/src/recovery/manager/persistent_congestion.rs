// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{path, recovery::SentPacketInfo};
use core::time::Duration;
use s2n_quic_core::{packet::number::PacketNumber, time::Timestamp};

pub(crate) struct PersistentCongestionCalculator {
    current_period: Option<PersistentCongestionPeriod>,
    max_duration: Duration,
    first_rtt_sample: Option<Timestamp>,
    path_id: path::Id,
}

impl PersistentCongestionCalculator {
    /// Create a new PersistentCongestionCalculator for the given `path_id`
    pub fn new(first_rtt_sample: Option<Timestamp>, path_id: path::Id) -> Self {
        Self {
            current_period: None,
            max_duration: Duration::ZERO,
            first_rtt_sample,
            path_id,
        }
    }

    /// Gets the longest persistent congestion period calculated
    pub fn persistent_congestion_duration(&self) -> Duration {
        self.max_duration
    }

    /// Called for each packet detected as lost
    pub fn on_lost_packet(&mut self, packet_number: PacketNumber, packet_info: &SentPacketInfo) {
        if self
            .first_rtt_sample
            .map_or(true, |ts| packet_info.time_sent < ts)
        {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
            //# The persistent congestion period SHOULD NOT start until there is at
            //# least one RTT sample.  Before the first RTT sample, a sender arms its
            //# PTO timer based on the initial RTT (Section 6.2.2), which could be
            //# substantially larger than the actual RTT.  Requiring a prior RTT
            //# sample prevents a sender from establishing persistent congestion with
            //# potentially too few probes.

            // The packet was sent prior to the first RTT sample, ignore it
            return;
        }

        // Check that this lost packet was sent on the same path
        //
        // Persistent congestion is only updated for the path on which we receive
        // an ack. Managing state for multiple paths requires extra allocations
        // but is only necessary when also attempting connection_migration; which
        // should not be very common.
        if packet_info.path_id != self.path_id {
            // The packet was sent on a different path, ignore it
            return;
        }

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
            self.current_period = Some(PersistentCongestionPeriod::new(
                packet_info.time_sent,
                packet_number,
            ));
        }
    }
}

struct PersistentCongestionPeriod {
    start: Timestamp,
    end: Timestamp,
    prev_packet: PacketNumber,
}

impl PersistentCongestionPeriod {
    /// Creates a new `PersistentCongestionPeriod`
    fn new(start: Timestamp, packet_number: PacketNumber) -> Self {
        Self {
            start,
            end: start,
            prev_packet: packet_number,
        }
    }

    /// True if the given packet number is 1 more than the last packet in this period
    fn is_contiguous(&self, packet_number: PacketNumber) -> bool {
        packet_number.checked_distance(self.prev_packet) == Some(1)
    }

    /// Extends this persistent congestion period
    fn extend(&mut self, packet_number: PacketNumber, packet_info: &SentPacketInfo) {
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
    fn duration(&self) -> Duration {
        self.end - self.start
    }
}
