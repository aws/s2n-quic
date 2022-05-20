// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::recovery::{
    bandwidth::{Bandwidth, PacketInfo, RateSample},
    bbr::{data_rate, data_volume, round},
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
    /// True if loss was encountered at any point in the current round trip
    loss_in_round: bool,
}

impl State {
    /// Updates the congestion state based on the latest delivery signals
    ///
    /// Called near the start of ACK processing
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        packet_info: PacketInfo,
        rate_sample: RateSample,
        delivered_bytes: u64,
        data_rate_model: &mut data_rate::Model,
        data_volume_model: &mut data_volume::Model,
        is_probing_bw: bool,
        cwnd: u32,
    ) {
        self.loss_round_counter.on_ack(packet_info, delivered_bytes);
        self.bw_latest = self.bw_latest.max(rate_sample.delivery_rate());
        self.inflight_latest = self.inflight_latest.max(rate_sample.delivered_bytes);

        data_rate_model.update_max_bw(rate_sample);

        if rate_sample.lost_bytes > 0 {
            self.loss_in_round = true;
        }

        if self.loss_round_counter.round_start() {
            if !is_probing_bw && self.loss_in_round {
                data_rate_model.update_lower_bound(self.bw_latest);
                data_volume_model.update_lower_bound(cwnd, self.inflight_latest);
            }

            self.loss_in_round = false;
        }
    }

    /// Initializes the congestion state for the next round
    ///
    /// Called near the end of ACK processing
    pub fn advance(&mut self, rate_sample: RateSample) {
        if self.loss_round_counter.round_start() {
            self.bw_latest = rate_sample.delivery_rate();
            self.inflight_latest = rate_sample.delivered_bytes;
        }
    }

    /// Resets the congestion signals
    #[allow(dead_code)] // TODO: Remove when used
    pub fn reset(&mut self) {
        self.loss_in_round = false;
        self.bw_latest = Bandwidth::ZERO;
        self.inflight_latest = 0;
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
        assert!(!state.loss_in_round);
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
        let mut data_volume_model = data_volume::Model::new(now);

        state.update(
            packet_info,
            rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
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
        let mut data_volume_model = data_volume::Model::new(now);

        state.update(
            packet_info,
            rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
        );

        assert!(state.loss_round_counter.round_start());
        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.max_bw());
        // Since there was loss in the round, the lower bounds are updated
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());
        // Loss in round is reset
        assert!(!state.loss_in_round);

        packet_info.delivered_bytes = 400;

        let new_rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 1000,
            lost_bytes: 50,
            ..Default::default()
        };

        state.update(
            packet_info,
            new_rate_sample,
            500,
            &mut data_rate_model,
            &mut data_volume_model,
            false,
            100,
        );

        assert!(!state.loss_round_counter.round_start());
        assert_eq!(new_rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(new_rate_sample.delivered_bytes, state.inflight_latest);
        assert_eq!(new_rate_sample.delivery_rate(), data_rate_model.max_bw());
        // It is not the start of a round, so lower bounds are not updated
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());
        assert!(state.loss_in_round);

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
        );

        assert!(state.loss_round_counter.round_start());
        // we are probing bw, so lower bounds should not update
        assert_eq!(rate_sample.delivery_rate(), data_rate_model.bw_lo());
        assert_eq!(rate_sample.delivered_bytes, data_volume_model.inflight_lo());
        // loss in round is still reset though
        assert!(!state.loss_in_round);
    }

    #[test]
    fn advance() {
        let mut state = State::default();

        let now = NoopClock.get_time();
        let packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        let mut rate_sample = RateSample {
            interval: Duration::from_millis(10),
            delivered_bytes: 100,
            ..Default::default()
        };

        state.update(
            packet_info,
            rate_sample,
            100,
            &mut data_rate::Model::new(),
            &mut data_volume::Model::new(now),
            false,
            100,
        );

        assert!(state.loss_round_counter.round_start());
        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);

        rate_sample.delivered_bytes = 500;
        state.advance(rate_sample);

        assert_eq!(rate_sample.delivery_rate(), state.bw_latest);
        assert_eq!(rate_sample.delivered_bytes, state.inflight_latest);
    }

    #[test]
    fn reset() {
        let mut state = State {
            loss_round_counter: Default::default(),
            loss_in_round: true,
            inflight_latest: 100,
            bw_latest: Bandwidth::MAX,
        };

        state.reset();

        assert!(!state.loss_in_round);
        assert_eq!(Bandwidth::ZERO, state.bw_latest);
        assert_eq!(0, state.inflight_latest);
    }
}
