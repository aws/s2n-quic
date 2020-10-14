use core::{
    cmp::{max, min},
    time::Duration,
};
use s2n_quic_core::time::Timestamp;

/// An implementation of the Hybrid Slow Start algorithm described in
/// "Hybrid Slow Start for High-Bandwidth and Long-Distance Networks"
/// https://pdfs.semanticscholar.org/25e9/ef3f03315782c7f1cbcd31b587857adae7d1.pdf
/// Most of the constants have been updated since this paper. This constants used in
/// this implementation are based on https://github.com/torvalds/linux/blob/net/ipv4/tcp_cubic.c
#[derive(Clone)]
pub struct HybridSlowStart {
    sample_count: usize,
    last_min_rtt: Option<Duration>,
    cur_min_rtt: Option<Duration>,
    threshold: usize,
    max_datagram_size: usize,
    time_of_last_packet: Option<Timestamp>,
    rtt_round_end_time: Option<Timestamp>,
}

/// Minimum slow start threshold in multiples of the max_datagram_size.
/// Defined as "hystart_low_window" in tcp_cubic.c
const LOW_SSTHRESH: usize = 16;
/// Number of samples required before determining the slow start threshold.
/// Defined as "HYSTART_MIN_SAMPLES" in tcp_cubic.c
const N_SAMPLING: usize = 8;
/// Minimum increase in delay to consider. Defined as"HYSTART_DELAY_MIN" in tcp_cubic.c
const MIN_DELAY_THRESHOLD: Duration = Duration::from_millis(4);
/// Maximum increase in delay to consider. Defined as"HYSTART_DELAY_MAX" in tcp_cubic.c
const MAX_DELAY_THRESHOLD: Duration = Duration::from_millis(16);
/// Factor for dividing the RTT to determine the threshold. Defined in tcp_cubic.c (not a constant)
const THRESHOLD_DIVIDEND: usize = 8;

impl HybridSlowStart {
    /// Constructs a new `HybridSlowStart`
    pub(super) fn new(max_datagram_size: usize) -> Self {
        Self {
            sample_count: 0,
            last_min_rtt: None,
            cur_min_rtt: None,
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.1
            //# A sender begins in slow start because the slow start threshold
            //# is initialized to an infinite value.
            threshold: usize::max_value(),
            max_datagram_size,
            time_of_last_packet: None,
            rtt_round_end_time: None,
        }
    }

    /// Get the current slow start threshold
    pub(super) fn threshold(&self) -> usize {
        self.threshold
    }

    pub(super) fn on_packet_sent(&mut self, time_sent: Timestamp) {
        self.time_of_last_packet = Some(time_sent);
    }

    /// Called each time the round trip time estimate is
    /// updated. The algorithm detects if the min RTT over
    /// a number of samples has increased since the last
    /// round of samples and if so will set the slow start
    /// threshold.
    pub(super) fn on_rtt_update(
        &mut self,
        congestion_window: usize,
        time_sent: Timestamp,
        rtt: Duration,
    ) {
        if congestion_window >= self.threshold {
            // We are already out of slow start so nothing to do
            return;
        }

        // An RTT round is over when a packet that was sent after the packet that
        // started the RTT round (or the starting packet itself) is acknowledged
        let rtt_round_is_over = self
            .rtt_round_end_time
            .map_or(true, |end_time| time_sent >= end_time);

        if rtt_round_is_over {
            // Start a new round and save the previous min RTT
            self.last_min_rtt = self.cur_min_rtt;
            self.cur_min_rtt = None;
            self.sample_count = 0;
            // End this round when packets sent after the current last packet
            // start getting acknowledged.
            self.rtt_round_end_time = self.time_of_last_packet;
        }

        if self.sample_count < N_SAMPLING {
            // Samples the delay, saving the minimum
            self.cur_min_rtt = Some(min(rtt, self.cur_min_rtt.unwrap_or(rtt)));
            self.sample_count += 1;
        }

        // We've gathered enough samples and there have been at least 2 RTT rounds
        // to compare, so check if the delay has increased between the rounds
        if let (N_SAMPLING, Some(last_min_rtt), Some(cur_min_rtt)) =
            (self.sample_count, self.last_min_rtt, self.cur_min_rtt)
        {
            let threshold =
                Duration::from_nanos((last_min_rtt.as_nanos() / THRESHOLD_DIVIDEND as u128) as u64);
            // Clamp n to the min and max thresholds
            let threshold = max(min(threshold, MAX_DELAY_THRESHOLD), MIN_DELAY_THRESHOLD);
            let delay_increase_is_over_threshold = cur_min_rtt >= last_min_rtt + threshold;
            let congestion_window_is_above_minimum = congestion_window >= self.low_ssthresh();

            if delay_increase_is_over_threshold && congestion_window_is_above_minimum {
                self.threshold = congestion_window;
            }
        }
    }

    /// Called when a congestion event is experienced. Sets the
    /// slow start threshold to the minimum of the Hybrid Slow Start threshold
    /// and the given congestion window. This will ensure we exit slow start
    /// early enough to avoid further congestion.
    pub(super) fn on_congestion_event(&mut self, ssthresh: usize) {
        self.threshold = max(self.low_ssthresh(), min(self.threshold, ssthresh));
    }

    fn low_ssthresh(&self) -> usize {
        LOW_SSTHRESH * self.max_datagram_size
    }
}
