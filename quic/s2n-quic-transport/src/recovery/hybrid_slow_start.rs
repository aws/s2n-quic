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
    found: bool,
    sampling_cnt: usize,
    last_min_rtt: Option<Duration>,
    cur_min_rtt: Option<Duration>,
    threshold: usize,
    max_datagram_size: usize,
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
    pub fn new(max_datagram_size: usize) -> Self {
        Self {
            found: false,
            sampling_cnt: 0,
            last_min_rtt: None,
            cur_min_rtt: None,
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.1
            //# A sender begins in slow start because the slow start threshold
            //# is initialized to an infinite value.
            threshold: usize::max_value(),
            max_datagram_size,
        }
    }

    /// Get the current slow start threshold
    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub(crate) fn on_packet_sent(&mut self, time_sent: Timestamp, sent_bytes: usize) {
        //TODO
    }

    /// Called each time the round trip time estimate is
    /// updated. The algorithm detects if the min RTT over
    /// a number of samples has increased since the last
    /// round of samples and if so will set the slow start
    /// threshold.
    pub fn on_rtt_update(&mut self, cwnd: usize, rtt: Duration) {
        if !self.found && cwnd <= self.threshold {
            if self.sampling_cnt == 0 {
                // Save the start of an RTT round
                self.last_min_rtt = self.cur_min_rtt;
                self.cur_min_rtt = Some(rtt);
                self.sampling_cnt = N_SAMPLING;
            } else {
                // Samples the delay, saving the minimum
                self.cur_min_rtt = Some(self.cur_min_rtt.map_or(rtt, |cur_rtt| cur_rtt.min(rtt)));
                self.sampling_cnt -= 1;
            }

            // The round is over
            if let (0, Some(last_min_rtt), Some(cur_min_rtt)) =
                (self.sampling_cnt, self.last_min_rtt, self.cur_min_rtt)
            {
                let n = Duration::from_nanos(
                    (last_min_rtt.as_nanos() / THRESHOLD_DIVIDEND as u128) as u64,
                );
                // Clamp n to the min and max thresholds
                let n = max(min(n, MAX_DELAY_THRESHOLD), MIN_DELAY_THRESHOLD);
                // If the delay increase is over n
                if cur_min_rtt >= last_min_rtt + n {
                    self.found = true;
                }

                if self.found && cwnd >= self.low_ssthresh() {
                    self.threshold = cwnd;
                } else {
                    // The found threshold is too low, keep searching
                    self.found = false;
                }
            }
        }
    }

    /// Called when a congestion event is experienced. Sets the
    /// slow start threshold to the minimum of the Hybrid Slow Start threshold
    /// and the given congestion window. This will ensure we exit slow start
    /// early enough to avoid further congestion. Found is reset so if the
    /// congestion window is decreased to below the slow start threshold, a new
    /// hybrid slow start threshold can be found.
    pub fn on_congestion_event(&mut self, ssthresh: usize) {
        self.threshold = max(self.low_ssthresh(), min(self.threshold, ssthresh));
        self.found = false;
    }

    fn low_ssthresh(&self) -> usize {
        LOW_SSTHRESH * self.max_datagram_size
    }
}
