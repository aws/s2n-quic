// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{path, recovery::SentPacketInfo};
use s2n_quic_core::{packet::number::PacketNumber, time::Timestamp};
use std::time::Duration;

pub(crate) struct PersistentCongestionCalculator {
    start: Option<Timestamp>,
    end: Option<Timestamp>,
    prev_lost_packet: Option<PacketNumber>,
    persistent_congestion_period: Duration,
    first_rtt_sample: Option<Timestamp>,
    path_id: path::Id,
}

impl PersistentCongestionCalculator {
    /// Create a new PersistentCongestionCalculator for the given `path_id`
    pub fn new(first_rtt_sample: Option<Timestamp>, path_id: path::Id) -> Self {
        Self {
            start: None,
            end: None,
            prev_lost_packet: None,
            persistent_congestion_period: Duration::ZERO,
            first_rtt_sample,
            path_id,
        }
    }

    /// Called for each packet detected as lost
    pub fn on_lost_packet(&mut self, packet_number: PacketNumber, packet_info: &SentPacketInfo) {
        if self
            .first_rtt_sample
            .map_or(true, |ts| packet_info.time_sent < ts)
        {
            //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.6.2
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

        let is_ack_eliciting = packet_info.ack_elicitation.is_ack_eliciting();

        if let (Some(start), Some(ref mut end)) = (self.start, self.end) {
            // We are current tracking a persistent congestion period

            //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.6.2
            //# A sender establishes persistent congestion after the receipt of an
            //# acknowledgment if two packets that are ack-eliciting are declared
            //# lost, and:
            //#
            //#     *  across all packet number spaces, none of the packets sent between
            //#        the send times of these two packets are acknowledged;

            // Check if this lost packet is contiguous with the previous lost packet.
            let is_contiguous = self
                .prev_lost_packet
                .map_or(false, |pn| packet_number.checked_distance(pn) == Some(1));

            if is_contiguous {
                if is_ack_eliciting {
                    // Extend the end of the current persistent congestion period

                    //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.6.2
                    //# These two packets MUST be ack-eliciting, since a receiver is required
                    //# to acknowledge only ack-eliciting packets within its maximum
                    //# acknowledgment delay; see Section 13.2 of [QUIC-TRANSPORT].
                    *end = packet_info.time_sent;
                }

                let persistent_congestion_period = *end - start;
                self.persistent_congestion_period = self
                    .persistent_congestion_period
                    .max(persistent_congestion_period);
            } else {
                // The current persistent congestion period has ended
                self.start = None;
                self.end = None;
            }
        }

        if self.start.is_none() && is_ack_eliciting {
            // Start tracking a new persistent congestion period

            //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.6.2
            //# These two packets MUST be ack-eliciting, since a receiver is required
            //# to acknowledge only ack-eliciting packets within its maximum
            //# acknowledgment delay; see Section 13.2 of [QUIC-TRANSPORT].
            self.start = Some(packet_info.time_sent);
            self.end = Some(packet_info.time_sent);
        }
        self.prev_lost_packet = Some(packet_number);
    }

    /// Gets the longest persistent congestion period calculated
    pub fn persistent_congestion_period(&self) -> Duration {
        self.persistent_congestion_period
    }
}
