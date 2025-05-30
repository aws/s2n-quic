// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;
use core::time::Duration;
#[cfg(not(feature = "std"))]
use num_traits::Float as _;

/// An implementation of the Hybrid Slow Start algorithm described in
/// "Hybrid Slow Start for High-Bandwidth and Long-Distance Networks"
/// https://pdfs.semanticscholar.org/25e9/ef3f03315782c7f1cbcd31b587857adae7d1.pdf
/// Most of the constants have been updated since this paper. This constants used in
/// this implementation are based on https://github.com/torvalds/linux/blob/net/ipv4/tcp_cubic.c
#[derive(Clone, Debug)]
pub struct HybridSlowStart {
    sample_count: usize,
    last_min_rtt: Option<Duration>,
    cur_min_rtt: Option<Duration>,
    pub(super) threshold: f32,
    max_datagram_size: u16,
    rtt_round_end_time: Option<Timestamp>,
    use_hystart_plus_plus: bool,
    ss_growth_divisor: f32,
    css_count: usize,
    css_baseline_min_rtt: Duration,
    css_threshold: f32,
}

/// Minimum slow start threshold in multiples of the max_datagram_size.
/// Defined as "hystart_low_window" in tcp_cubic.c
const LOW_SSTHRESH: f32 = 16.0;
/// Factor for dividing the RTT to determine the threshold. Defined in tcp_cubic.c (not a constant)
//= https://tools.ietf.org/id/draft-ietf-tcpm-hystartplusplus-04.txt#section-4.2
//#   o  RttThresh = clamp(MIN_RTT_THRESH, lastRoundMinRTT / 8, MAX_RTT_THRESH)
const THRESHOLD_DIVIDEND: u32 = 8;

//= https://tools.ietf.org/id/draft-ietf-tcpm-hystartplusplus-04.txt#section-4.3
//# It is RECOMMENDED that a HyStart++ implementation use the following
//# constants:
//#
//# *  MIN_RTT_THRESH = 4 msec
//#
//# *  MAX_RTT_THRESH = 16 msec
//#
//# *  N_RTT_SAMPLE = 8
//#
//# *  CSS_GROWTH_DIVISOR = 4
//#
//# *  CSS_ROUNDS = 5

/// Number of samples required before determining the slow start threshold.
/// Defined as "HYSTART_MIN_SAMPLES" in tcp_cubic.c
const N_SAMPLING: usize = 8;
/// Minimum increase in delay to consider. Defined as"HYSTART_DELAY_MIN" in tcp_cubic.c
const MIN_DELAY_THRESHOLD: Duration = Duration::from_millis(4);
/// Maximum increase in delay to consider. Defined as"HYSTART_DELAY_MAX" in tcp_cubic.c
const MAX_DELAY_THRESHOLD: Duration = Duration::from_millis(16);
/// Growth devisor for CSS phase
const CSS_GROWTH_DIVISOR: f32 = 4.0;
/// Maximum rounds for CSS phase
const CSS_ROUNDS: usize = 5;
/// environment variable for using hystart++
#[cfg(feature = "std")]
const USE_HYSTART_PLUS_PLUS: &str = "S2N_UNSTABLE_USE_HYSTART_PP";

impl HybridSlowStart {
    /// Constructs a new `HybridSlowStart`. `max_datagram_size` is used for determining
    /// the minimum slow start threshold.
    pub fn new(max_datagram_size: u16) -> Self {
        Self {
            sample_count: 0,
            last_min_rtt: None,
            cur_min_rtt: None,
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.1
            //# A sender begins in slow start because the slow start threshold
            //# is initialized to an infinite value.
            threshold: f32::MAX,
            max_datagram_size,
            rtt_round_end_time: None,
            use_hystart_plus_plus: Self::use_hystart_parameter(),
            ss_growth_divisor: 1.0,
            css_count: 0,
            css_baseline_min_rtt: Duration::ZERO,
            css_threshold: f32::MAX,
        }
    }

    /// Called each time the round trip time estimate is
    /// updated. The algorithm detects if the min RTT over
    /// a number of samples has increased since the last
    /// round of samples and if so will set the slow start
    /// threshold.
    pub fn on_rtt_update(
        &mut self,
        congestion_window: f32,
        time_sent: Timestamp,
        time_of_last_sent_packet: Timestamp,
        rtt: Duration,
    ) {
        let ss_threshold_found = self.threshold < f32::MAX;
        if congestion_window >= self.threshold || (self.use_hystart_plus_plus && ss_threshold_found)
        {
            //= https://tools.ietf.org/id/draft-ietf-tcpm-hystartplusplus-04.txt#section-4.3
            //# An implementation SHOULD use HyStart++ only for the initial slow
            //# start (when ssthresh is at its initial value of arbitrarily high per
            //# [RFC5681]) and fall back to using traditional slow start for the
            //# remainder of the connection lifetime.
            return;
        }

        // An RTT round is over when a packet that was sent after the packet that
        // started the RTT round (or the starting packet itself) is acknowledged
        let rtt_round_is_over = self
            .rtt_round_end_time
            .is_none_or(|end_time| time_sent >= end_time);

        if rtt_round_is_over {
            // Start a new round and save the previous min RTT
            self.last_min_rtt = self.cur_min_rtt;
            self.cur_min_rtt = None;
            self.sample_count = 0;
            // End this round when packets sent after the current last sent packet
            // start getting acknowledged.
            self.rtt_round_end_time = Some(time_of_last_sent_packet);
        }

        if self.sample_count < N_SAMPLING {
            // Sample the delay, saving the minimum
            self.cur_min_rtt = Some(rtt.min(self.cur_min_rtt.unwrap_or(rtt)));
        }

        self.sample_count += 1;

        // We've gathered enough samples and there have been at least 2 RTT rounds
        // to compare, so check if the delay has increased between the rounds
        if let (N_SAMPLING, Some(last_min_rtt), Some(cur_min_rtt)) =
            (self.sample_count, self.last_min_rtt, self.cur_min_rtt)
        {
            if congestion_window >= self.css_threshold {
                self.css_count += 1;
                if cur_min_rtt < self.css_baseline_min_rtt {
                    // resume slow start
                    self.css_threshold = self.threshold;
                    self.ss_growth_divisor = 1.0;
                    self.css_count = 0;
                }
                if self.css_count >= CSS_ROUNDS {
                    // exit slow start phase
                    self.threshold = congestion_window;
                    self.css_threshold = f32::MAX;
                    self.ss_growth_divisor = 1.0;
                }
            } else {
                let threshold = last_min_rtt / THRESHOLD_DIVIDEND;
                // Clamp n to the min and max thresholds
                let threshold = threshold.min(MAX_DELAY_THRESHOLD).max(MIN_DELAY_THRESHOLD);
                let delay_increase_is_over_threshold = cur_min_rtt >= last_min_rtt + threshold;
                let congestion_window_is_above_minimum = congestion_window >= self.low_ssthresh();

                if self.use_hystart_plus_plus {
                    // if delay is beyond threshold, go into css phase
                    if delay_increase_is_over_threshold {
                        self.css_threshold = congestion_window;
                        self.css_baseline_min_rtt = cur_min_rtt;
                        self.ss_growth_divisor = CSS_GROWTH_DIVISOR;
                        self.css_count = 0;
                    }
                } else if delay_increase_is_over_threshold && congestion_window_is_above_minimum {
                    self.threshold = congestion_window;
                }
            }
        }
    }

    /// return cwnd increment during slow start phase
    /// should be called from on_packet_ack
    pub fn cwnd_increment(&self, sent_bytes: usize) -> f32 {
        if cfg!(debug_assertions) && !self.use_hystart_plus_plus {
            assert!((self.ss_growth_divisor - 1.0).abs() < f32::EPSILON);
        }
        (sent_bytes as f32) / self.ss_growth_divisor
    }

    /// Called when a congestion event is experienced. Sets the
    /// slow start threshold to the minimum of the Hybrid Slow Start threshold
    /// and the given congestion window. This will ensure we exit slow start
    /// early enough to avoid further congestion.
    pub fn on_congestion_event(&mut self, ssthresh: f32) {
        self.threshold = self.threshold.min(ssthresh).max(self.low_ssthresh());
        self.ss_growth_divisor = 1.0;
        self.css_threshold = f32::MAX;
    }

    fn low_ssthresh(&self) -> f32 {
        LOW_SSTHRESH * self.max_datagram_size as f32
    }

    #[cfg(feature = "std")]
    fn use_hystart_parameter() -> bool {
        use once_cell::sync::OnceCell;
        static USE_HYSTART_PP: OnceCell<bool> = OnceCell::new();
        *USE_HYSTART_PP.get_or_init(|| std::env::var(USE_HYSTART_PLUS_PLUS).is_ok())
    }

    #[cfg(not(feature = "std"))]
    fn use_hystart_parameter() -> bool {
        false
    }
}

#[cfg(test)]
mod test {
    use crate::{
        assert_delta,
        recovery::hybrid_slow_start::HybridSlowStart,
        time::{Clock, NoopClock},
    };
    use core::time::Duration;

    #[test]
    fn on_congestion_event() {
        let mut slow_start = HybridSlowStart::new(10);
        slow_start.threshold = 501.0;

        // Setting a threshold lower than the current threshold
        // will override the current threshold
        slow_start.on_congestion_event(500.0);
        assert_delta!(slow_start.threshold, 500.0, 0.001);

        slow_start.threshold = 501.0;

        // Setting a threshold higher than the current threshold
        // keeps the current threshold intact
        slow_start.on_congestion_event(502.0);
        assert_delta!(slow_start.threshold, 501.0, 0.001);

        slow_start.threshold = 501.0;

        // Setting a threshold lower than low_ssthresh
        // sets the threshold to low_ssthresh
        slow_start.on_congestion_event(slow_start.low_ssthresh() - 1.0);
        assert_delta!(slow_start.threshold, slow_start.low_ssthresh(), 0.001);
    }

    #[test]
    fn on_rtt_update_above_threshold() {
        let mut slow_start = HybridSlowStart::new(10);
        let time_zero = NoopClock.get_time();
        slow_start.threshold = 500.0;

        assert_eq!(slow_start.sample_count, 0);
        slow_start.on_rtt_update(750.0, time_zero, time_zero, Duration::from_secs(1));

        assert_delta!(slow_start.threshold, 500.0, 0.001);
        assert_eq!(slow_start.sample_count, 0);
    }

    #[test]
    fn on_rtt_update() {
        let mut slow_start = HybridSlowStart::new(10);

        assert_eq!(slow_start.sample_count, 0);

        let time_zero = NoopClock.get_time() + Duration::from_secs(10);

        // -- Round 1 --

        // t=0-9: Send packet #1-10
        let time_of_last_sent_packet = time_zero + Duration::from_millis(9);

        // t=10: Acknowledge packets #1-7 all with RTT 200
        for i in 0..=6 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(200),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(200)));
        }

        // This round will end when packet #10 is acknowledged
        assert_eq!(
            slow_start.rtt_round_end_time,
            Some(time_of_last_sent_packet)
        );

        // t=11: Acknowledge packet #8 with RTT 100
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(7),
            time_of_last_sent_packet,
            Duration::from_millis(100),
        );

        // The current minimum should now be 100
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(100)));

        // t=11: Acknowledge packet #9 with RTT 50
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(8),
            time_of_last_sent_packet,
            Duration::from_millis(50),
        );

        // The current minimum is still 100 because we've already collected N_SAMPLING samples
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(100)));

        // -- Round 2 --

        // t=20-29: Send packet #11-20
        let time_of_last_sent_packet = time_zero + Duration::from_millis(29);

        // t=30: Acknowledge packet #10 with RTT 400, ending the first round and starting the second
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(9),
            time_of_last_sent_packet,
            Duration::from_millis(400),
        );

        // The last minimum is saved in last_min_rtt
        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(100)));
        // The current minimum is now 400 because we've started a new round
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(400)));
        // This round will end when packet #20 is acknowledged
        assert_eq!(
            slow_start.rtt_round_end_time,
            Some(time_of_last_sent_packet)
        );

        // t=31: Acknowledge packets #11-16 all with RTT 500
        for i in 20..=25 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(400)));
        }

        // t=32: Acknowledge packet #17, the 8th sample in Round 2
        slow_start.on_rtt_update(
            2000.0,
            time_zero + Duration::from_millis(27),
            time_of_last_sent_packet,
            Duration::from_millis(112),
        );

        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(100)));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(112)));
        // The last ack was the 8th sample, but since the min rtt increased below the threshold,
        // the slow start threshold remains the same
        assert_delta!(slow_start.threshold, f32::MAX, 0.001);

        // -- Round 3 --

        // t=40-49: Send packet #21-30
        let time_of_last_sent_packet = time_zero + Duration::from_millis(49);

        // t=50: Acknowledge packets 21-27
        for i in 40..=46 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(500)));
        }
        // t=51: Acknowledge packet 28, the 8th sample in Round 3
        slow_start.on_rtt_update(
            5000.0,
            time_zero + Duration::from_millis(38),
            time_of_last_sent_packet,
            Duration::from_millis(126),
        );

        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(112)));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(126)));
        // The last ack was the 8th sample, and since the min rtt increased above the threshold,
        // the slow start threshold was set to the current congestion window
        assert_delta!(slow_start.threshold, 5000.0, 0.001);
    }

    #[test]
    fn on_rtt_update_with_hystartplus_1() {
        let mut slow_start = HybridSlowStart::new(10);
        // use hystart++
        slow_start.use_hystart_plus_plus = true;

        assert_eq!(slow_start.sample_count, 0);

        let time_zero = NoopClock.get_time() + Duration::from_secs(10);

        // -- Round 1 --

        // t=0-9: Send packet #1-10
        let time_of_last_sent_packet = time_zero + Duration::from_millis(9);

        // t=10: Acknowledge packets #1-7 all with RTT 200
        for i in 0..=6 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(200),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(200)));
        }

        // This round will end when packet #10 is acknowledged
        assert_eq!(
            slow_start.rtt_round_end_time,
            Some(time_of_last_sent_packet)
        );

        // t=11: Acknowledge packet #8 with RTT 100
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(7),
            time_of_last_sent_packet,
            Duration::from_millis(100),
        );

        // The current minimum should now be 100
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(100)));

        // t=11: Acknowledge packet #9 with RTT 50
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(8),
            time_of_last_sent_packet,
            Duration::from_millis(50),
        );

        // The current minimum is still 100 because we've already collected N_SAMPLING samples
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(100)));

        // -- Round 2 --

        // t=20-29: Send packet #11-20
        let time_of_last_sent_packet = time_zero + Duration::from_millis(29);

        // t=30: Acknowledge packet #10 with RTT 400, ending the first round and starting the second
        slow_start.on_rtt_update(
            1000.0,
            time_zero + Duration::from_millis(9),
            time_of_last_sent_packet,
            Duration::from_millis(400),
        );

        // The last minimum is saved in last_min_rtt
        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(100)));
        // The current minimum is now 400 because we've started a new round
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(400)));
        // This round will end when packet #20 is acknowledged
        assert_eq!(
            slow_start.rtt_round_end_time,
            Some(time_of_last_sent_packet)
        );

        // t=31: Acknowledge packets #11-16 all with RTT 500
        for i in 20..=25 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(400)));
        }

        // t=32: Acknowledge packet #17, the 8th sample in Round 2
        slow_start.on_rtt_update(
            2000.0,
            time_zero + Duration::from_millis(27),
            time_of_last_sent_packet,
            Duration::from_millis(112),
        );

        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(100)));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(112)));
        // The last ack was the 8th sample, but since the min rtt increased below the threshold,
        // the slow start threshold remains the same
        assert_delta!(slow_start.threshold, f32::MAX, 0.001);

        // -- Round 3 --

        // t=40-49: Send packet #21-30
        let time_of_last_sent_packet = time_zero + Duration::from_millis(49);

        // t=50: Acknowledge packets 21-27
        for i in 40..=46 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(500)));
        }
        // t=51: Acknowledge packet 28, the 8th sample in Round 3
        slow_start.on_rtt_update(
            5000.0,
            time_zero + Duration::from_millis(38),
            time_of_last_sent_packet,
            Duration::from_millis(126),
        );

        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(112)));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(126)));
        // The last ack was the 8th sample, and since the min rtt increased above the threshold,
        // the slow start cssthreshold was set to the current congestion window
        // Also, cssbaseline is cur_min_rtt and ss_growth_divisor is 4.0
        assert_delta!(slow_start.css_threshold, 5000.0, 0.001);
        assert_eq!(slow_start.css_baseline_min_rtt, Duration::from_millis(126));
        assert_delta!(slow_start.ss_growth_divisor, 4.0, 0.001);

        // -- Round 4 --
        // t=50-59: Send packet #31-40
        // first round for CSS
        let time_of_last_sent_packet = time_zero + Duration::from_millis(69);

        // t=60: Acknowledge packets 31-37
        for i in 60..=66 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(500)));
        }

        // t=51: Acknowledge packet 38, the 8th sample in Round 4
        slow_start.on_rtt_update(
            5000.0,
            time_zero + Duration::from_millis(38),
            time_of_last_sent_packet,
            Duration::from_millis(130),
        );

        let cwnd_increment = slow_start.cwnd_increment(1000);

        assert_delta!(cwnd_increment, 250.0, 0.001);
        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(126)));
        assert_eq!(slow_start.css_baseline_min_rtt, Duration::from_millis(126));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(130)));
        // because the cur_min_rtt is bigger than css_baseline_min_rtt, CSS phase should continue
        assert_eq!(slow_start.css_count, 1);
    }

    #[test]
    fn on_rtt_update_with_hystartplus_2() {
        let mut slow_start = HybridSlowStart::new(10);
        // use hystart++
        slow_start.use_hystart_plus_plus = true;

        // emulate Round 1 and Round 2
        let time_zero = NoopClock.get_time() + Duration::from_secs(10);
        slow_start.cur_min_rtt = Some(Duration::from_millis(112));

        // -- Round 3 --

        // t=40-49: Send packet #21-30
        let time_of_last_sent_packet = time_zero + Duration::from_millis(49);

        // t=50: Acknowledge packets 21-27
        for i in 40..=46 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(500)));
        }
        // t=51: Acknowledge packet 28, the 8th sample in Round 3
        slow_start.on_rtt_update(
            5000.0,
            time_zero + Duration::from_millis(38),
            time_of_last_sent_packet,
            Duration::from_millis(126),
        );

        //lassert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(112)));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(126)));
        // The last ack was the 8th sample, and since the min rtt increased above the threshold,
        // the slow start cssthreshold was set to the current congestion window
        // Also, cssbaseline is cur_min_rtt and ss_growth_divisor is 4.0
        assert_delta!(slow_start.css_threshold, 5000.0, 0.001);
        assert_eq!(slow_start.css_baseline_min_rtt, Duration::from_millis(126));
        assert_delta!(slow_start.ss_growth_divisor, 4.0, 0.001);

        // -- Round 4 --
        // t=50-59: Send packet #31-40
        // first round for CSS
        let time_of_last_sent_packet = time_zero + Duration::from_millis(69);

        // t=60: Acknowledge packets 31-37
        for i in 60..=66 {
            slow_start.on_rtt_update(
                1000.0,
                time_zero + Duration::from_millis(i),
                time_of_last_sent_packet,
                Duration::from_millis(500),
            );
            assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(500)));
        }

        // t=51: Acknowledge packet 38, the 8th sample in Round 4
        slow_start.on_rtt_update(
            5000.0,
            time_zero + Duration::from_millis(38),
            time_of_last_sent_packet,
            Duration::from_millis(125),
        );

        assert_eq!(slow_start.last_min_rtt, Some(Duration::from_millis(126)));
        assert_eq!(slow_start.css_baseline_min_rtt, Duration::from_millis(126));
        assert_eq!(slow_start.cur_min_rtt, Some(Duration::from_millis(125)));
        // because the cur_min_rtt is smaller than css_baseline_min_rtt, should resume slow start
        assert_delta!(slow_start.ss_growth_divisor, 1.0, 0.001);
        assert_delta!(slow_start.css_threshold, f32::MAX, 0.001);
        assert_eq!(slow_start.css_count, 0);
    }
}
