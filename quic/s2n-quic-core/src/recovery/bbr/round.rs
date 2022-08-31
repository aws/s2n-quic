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
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.1
    //# BBRInitRoundCounting():
    //#   BBR.next_round_delivered = 0
    //#   BBR.round_start = false
    //#   BBR.round_count = 0
    /// The `delivered_bytes` at which the next round begins
    next_round_delivered_bytes: u64,
    /// True if the current ack being processed started a new round
    round_start: bool,
    /// The number of rounds counted since initialization
    round_count: u64,
}

impl Counter {
    /// Called for each acknowledgement of one or more packets
    pub fn on_ack(&mut self, packet_info: PacketInfo, delivered_bytes: u64) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.1
        //# BBRUpdateRound():
        //#   if (packet.delivered >= BBR.next_round_delivered)
        //#     BBRStartRound()
        //#     BBR.round_count++
        //#     BBR.rounds_since_probe++
        //#     BBR.round_start = true
        //#   else
        //#     BBR.round_start = false
        if packet_info.delivered_bytes >= self.next_round_delivered_bytes {
            self.set_round_end(delivered_bytes);
            self.round_count += 1;
            self.round_start = true;
        } else {
            self.round_start = false;
        }
    }

    /// Sets the end of the current round to the given `delivered_bytes`
    pub fn set_round_end(&mut self, delivered_bytes: u64) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.1
        //# BBRStartRound():
        //#   BBR.next_round_delivered = C.delivered

        debug_assert!(
            delivered_bytes >= self.next_round_delivered_bytes,
            "The end of the round can only be extended, not shortened"
        );
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

    #[cfg(test)]
    pub fn round_end(&self) -> u64 {
        self.next_round_delivered_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, NoopClock};

    #[test]
    fn counter() {
        let mut counter = Counter::default();

        assert!(!counter.round_start());
        assert_eq!(0, counter.round_count());

        let now = NoopClock.get_time();
        let mut packet_info = PacketInfo {
            delivered_bytes: 0,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };

        let round_end_at = 100;
        counter.on_ack(packet_info, round_end_at);

        assert!(counter.round_start());
        assert_eq!(1, counter.round_count());

        let mut delivered_bytes = round_end_at;

        for i in 0..round_end_at {
            packet_info.delivered_bytes = i;
            delivered_bytes += packet_info.delivered_bytes;
            counter.on_ack(packet_info, delivered_bytes);

            // No new round started since we haven't reach the round end
            assert!(!counter.round_start());
            assert_eq!(1, counter.round_count());
        }

        // Now we have reached the end of the round
        packet_info.delivered_bytes = round_end_at;
        delivered_bytes += round_end_at;

        counter.on_ack(packet_info, delivered_bytes);

        assert!(counter.round_start());
        assert_eq!(2, counter.round_count());
    }

    #[test]
    fn set_round_end() {
        let mut counter = Counter::default();

        assert!(!counter.round_start());
        assert_eq!(0, counter.round_count());

        let now = NoopClock.get_time();
        let mut packet_info = PacketInfo {
            delivered_bytes: 0,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };

        let round_end_at = 100;
        counter.on_ack(packet_info, round_end_at);

        assert!(counter.round_start());
        assert_eq!(1, counter.round_count());

        packet_info.delivered_bytes += 1;

        counter.on_ack(packet_info, round_end_at);

        assert!(!counter.round_start());
        assert_eq!(1, counter.round_count());

        let new_round_end = round_end_at + 1;
        counter.set_round_end(new_round_end);

        // `set_round_end` does not start a new round
        assert!(!counter.round_start());
        assert_eq!(1, counter.round_count());

        packet_info.delivered_bytes = round_end_at;
        counter.on_ack(packet_info, round_end_at);

        // Since the end of the round has been updated, a new round is not started
        // when the original round end has been reached
        assert!(!counter.round_start());
        assert_eq!(1, counter.round_count());

        packet_info.delivered_bytes = new_round_end;
        counter.on_ack(packet_info, new_round_end + 100);

        // We've reached the new round end, so a new round is started
        assert!(counter.round_start());
        assert_eq!(2, counter.round_count());
    }
}
