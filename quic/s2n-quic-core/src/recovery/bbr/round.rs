// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::recovery::bandwidth::PacketInfo;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.1
//# Several aspects of the BBR algorithm depend on counting the progress of "packet-timed" round
//# trips, which start at the transmission of some segment, and then end at the acknowledgement
//# of that segment. BBR.round_count is a count of the number of these "packet-timed" round trips
//# elapsed so far.
#[derive(Clone, Debug, Default)]
pub(crate) struct Counter {
    /// The `delivered_bytes` at which the next round begins
    next_round_delivered_bytes: u64,
    /// True if the current ack being processed started a new round
    round_start: bool,
    /// The number of rounds counted since initialization
    round_count: u64,
}
#[allow(dead_code)] // TODO: Remove when used
impl Counter {
    /// Called for each acknowledgement of one or more packets
    pub fn on_ack(&mut self, packet_info: PacketInfo, delivered_bytes: u64) {
        if packet_info.delivered_bytes >= self.next_round_delivered_bytes {
            self.start(delivered_bytes);
            self.round_count += 1;
            self.round_start = true;
        } else {
            self.round_start = false;
        }
    }

    /// Starts a round that ends when the packet sent with `delivered_bytes` is acked
    pub fn start(&mut self, delivered_bytes: u64) {
        self.next_round_delivered_bytes = delivered_bytes;
    }

    /// True if the latest acknowledgement started a new round, false otherwise
    pub fn round_start(&self) -> bool {
        self.round_start
    }

    /// The number of rounds counted since initialization
    pub fn round_count(&self) -> u64 {
        self.round_count
    }
}
