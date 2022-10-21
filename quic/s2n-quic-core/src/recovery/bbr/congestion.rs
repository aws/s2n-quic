// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::{
        bandwidth::{Bandwidth, PacketInfo, RateSample},
        bbr::{data_rate, data_volume, round, BbrCongestionController},
    },
};

#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    /// Tracks round trips for ensuring BBR reacts to congestion only once per round
    loss_round_counter: round::Counter,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.10
    //# a 1-round-trip max of delivered bandwidth (rs.delivery_rate)
    bw_latest: Bandwidth,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.10
    //# a 1-round-trip max of delivered volume of data (rs.delivered)
    inflight_latest: u64,
    /// The number of bursts of loss encountered during the current round trip
    loss_bursts_in_round: Counter<u8, Saturating>,
    /// True if packets were marked with ECN CE at any point in the current round trip
    ecn_in_round: bool,
}

impl State {
    /// Updates the congestion state based on the latest delivery signals
    ///
    /// Called near the start of ACK processing
    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        packet_info: PacketInfo,
        rate_sample: RateSample,
        delivered_bytes: u64,
        data_rate_model: &mut data_rate::Model,
        data_volume_model: &mut data_volume::Model,
        is_probing_for_bandwidth: bool,
        cwnd: u32,
        ecn_alpha: f64,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
        //# BBRUpdateLatestDeliverySignals():
        //#   BBR.loss_round_start = 0
        //#   BBR.bw_latest       = max(BBR.bw_latest,       rs.delivery_rate)
        //#   BBR.inflight_latest = max(BBR.inflight_latest, rs.delivered)
        //#   if (rs.prior_delivered >= BBR.loss_round_delivered)
        //#     BBR.loss_round_delivered = C.delivered
        //#     BBR.loss_round_start = 1

        self.loss_round_counter.on_ack(packet_info, delivered_bytes);
        self.bw_latest = self.bw_latest.max(rate_sample.delivery_rate());
        self.inflight_latest = self.inflight_latest.max(rate_sample.delivered_bytes);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
        //# BBRUpdateCongestionSignals():
        //#   BBRUpdateMaxBw()
        //#   if (rs.losses > 0)
        //#     BBR.loss_in_round = 1
        //#   if (!BBR.loss_round_start)
        //#     return  /* wait until end of round trip */
        //#   BBRAdaptLowerBoundsFromCongestion()
        //#   BBR.loss_in_round = 0

        data_rate_model.update_max_bw(rate_sample);

        if rate_sample.ecn_ce_count > 0 {
            self.ecn_in_round = true;
        }

        if self.loss_round_counter.round_start() && !is_probing_for_bandwidth {
            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
            //# When not explicitly accelerating to probe for bandwidth (Drain, ProbeRTT,
            //# ProbeBW_DOWN, ProbeBW_CRUISE), BBR responds to loss by slowing down to some extent.

            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
            //# BBRAdaptLowerBoundsFromCongestion():
            //#   if (BBRIsProbingBW())
            //#     return
            //#   if (BBR.loss_in_round())
            //#     BBRInitLowerBounds()
            //#     BBRLossLowerBounds()

            if self.loss_in_round() {
                // The following update_lower_bound method combines the functionality of
                // BBRInitLowerBounds() and BBRLossLowerBounds()
                data_rate_model.update_lower_bound(self.bw_latest);
            }

            // Update inflight_lo to the lower of the ECN and the Loss based values
            // if there is loss or ECN in the round
            data_volume_model.update_lower_bound(
                cwnd,
                self.inflight_latest,
                self.loss_in_round(),
                self.ecn_in_round,
                ecn_alpha,
            );
        }
    }

    /// Initializes the congestion state for the next round
    ///
    /// Called near the end of ACK processing
    pub(super) fn advance(&mut self, rate_sample: RateSample) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
        //# BBRAdvanceLatestDeliverySignals():
        //#   if (BBR.loss_round_start)
        //#     BBR.bw_latest       = rs.delivery_rate
        //#     BBR.inflight_latest = rs.delivered
        if self.loss_round_counter.round_start() {
            self.bw_latest = rate_sample.delivery_rate();
            self.inflight_latest = rate_sample.delivered_bytes;
            self.loss_bursts_in_round = Default::default();
            self.ecn_in_round = false;
        }
    }

    /// Resets the congestion signals
    pub(super) fn reset(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
        //# BBRResetCongestionSignals():
        //#   BBR.loss_in_round = 0
        //#   BBR.bw_latest = 0
        //#   BBR.inflight_latest = 0
        self.loss_bursts_in_round = Default::default();
        self.ecn_in_round = false;
        self.bw_latest = Bandwidth::ZERO;
        self.inflight_latest = 0;
    }

    #[inline]
    pub(super) fn on_packet_lost(&mut self, delivered_bytes: u64, new_lost_burst: bool) {
        if !self.loss_in_round() {
            self.loss_round_counter.set_round_end(delivered_bytes);
        }

        if new_lost_burst {
            self.loss_bursts_in_round += 1;
        }
    }

    #[inline]
    pub(super) fn on_explicit_congestion(&mut self) {
        self.ecn_in_round = true;
    }

    #[inline]
    /// Returns true if this is the beginning of a new loss round
    pub(super) fn loss_round_start(&self) -> bool {
        self.loss_round_counter.round_start()
    }

    #[inline]
    /// Returns true if there was loss in the current round
    pub(super) fn loss_in_round(&self) -> bool {
        self.loss_bursts_in_round > 0
    }

    #[inline]
    /// Returns the number of loss busts in the current round
    pub(super) fn loss_bursts_in_round(&self) -> u8 {
        *self.loss_bursts_in_round
    }

    #[inline]
    /// Returns true if there were ECN CE marked packets in the current round
    pub(super) fn ecn_in_round(&self) -> bool {
        self.ecn_in_round
    }
}

/// Methods related to Congestion state
impl BbrCongestionController {
    /// Updates delivery and congestion signals according to
    /// BBRUpdateLatestDeliverySignals() and BBRUpdateCongestionSignals()
    #[inline]
    pub(super) fn update_latest_signals(&mut self, packet_info: PacketInfo) {
        self.congestion_state.update(
            packet_info,
            self.bw_estimator.rate_sample(),
            self.bw_estimator.delivered_bytes(),
            &mut self.data_rate_model,
            &mut self.data_volume_model,
            self.state.is_probing_for_bandwidth(),
            self.cwnd,
            self.ecn_state.alpha(),
        );
    }
}

#[cfg(test)]
pub mod testing {
    use crate::{
        recovery::{
            bandwidth::{Bandwidth, PacketInfo, RateSample},
            bbr::{congestion, data_rate, data_volume},
        },
        time::{Clock, NoopClock},
    };
    use std::time::Duration;

    /// Asserts that the given `congestion::State` has been reset
    pub(crate) fn assert_reset(state: congestion::State) {
        assert!(!state.loss_in_round());
        assert!(!state.ecn_in_round);
        assert_eq!(Bandwidth::ZERO, state.bw_latest);
        assert_eq!(0, state.inflight_latest);
    }

    /// Return congestion::State updated with data
    pub(crate) fn test_state() -> congestion::State {
        let mut state = congestion::State::default();

        let now = NoopClock.get_time();
        let packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        let rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 100,
            lost_bytes: 50,
            ..Default::default()
        };
        let mut data_rate_model = data_rate::Model::new();
        let mut data_volume_model = data_volume::Model::new();

        state.update(
            packet_info,
            rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
            1.0,
        );

        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, NoopClock};
    use core::time::Duration;

    #[test]
    fn update() {
        let mut state = State::default();

        let now = NoopClock.get_time();
        let mut packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        let rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 100,
            lost_bytes: 50,
            ..Default::default()
        };
        let mut data_rate_model = data_rate::Model::new();
        let mut data_volume_model = data_volume::Model::new();

        state.on_packet_lost(100, true);
        state.update(
            packet_info,
            rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
            1.0,
        );

        assert!(state.loss_round_counter.round_start());
        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.max_bw());
        // Since there was loss in the round, the lower bounds are updated
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());

        state.advance(rate_sample);
        // Loss and ecn in round are reset
        assert!(!state.loss_in_round());
        assert!(!state.ecn_in_round);

        packet_info.delivered_bytes = 400;

        let new_rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 1000,
            lost_bytes: 50,
            ecn_ce_count: 5,
            ..Default::default()
        };

        state.on_packet_lost(500, true);
        state.update(
            packet_info,
            new_rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
            1.0,
        );

        assert!(!state.loss_round_counter.round_start());
        assert_eq!(new_rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(new_rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(new_rate_sample.delivery_rate(), data_rate_model.max_bw());
        // It is not the start of a round, so lower bounds are not updated
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());
        assert!(state.loss_in_round());
        assert!(state.ecn_in_round);

        // This packet ends the rounds
        packet_info.delivered_bytes = 500;

        state.update(
            packet_info,
            new_rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            true, // we are probing bw, so lower bounds should not update
            100,
            1.0,
        );

        assert!(state.loss_round_counter.round_start());
        // we are probing bw, so lower bounds should not update
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());
        // loss and ecn in round are still reset though
        state.advance(rate_sample);
        assert!(!state.loss_in_round());
        assert!(!state.ecn_in_round);
    }

    #[test]
    fn advance() {
        let mut state = State::default();

        let now = NoopClock.get_time();
        let packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        let mut rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 100,
            ..Default::default()
        };

        state.on_packet_lost(100, true);
        state.on_packet_lost(100, true);
        state.on_explicit_congestion();
        state.update(
            packet_info,
            rate_sample,
            100,
            &mut data_rate::Model::new(),
            &mut data_volume::Model::new(),
            false,
            100,
            1.0,
        );

        assert!(state.loss_round_counter.round_start());
        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(2, state.loss_bursts_in_round());
        assert!(state.ecn_in_round);

        rate_sample.delivered_bytes = 500;
        state.advance(rate_sample);

        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(0, state.loss_bursts_in_round());
        assert!(!state.ecn_in_round);
    }

    #[test]
    fn reset() {
        let mut state = State {
            loss_round_counter: Default::default(),
            loss_bursts_in_round: Counter::new(10),
            inflight_latest: 100,
            bw_latest: Bandwidth::INFINITY,
            ecn_in_round: true,
        };

        state.reset();

        assert!(!state.loss_in_round());
        assert!(!state.ecn_in_round);
        assert_eq!(Bandwidth::ZERO, state.bw_latest);
        assert_eq!(0, state.inflight_latest);
    }
}
