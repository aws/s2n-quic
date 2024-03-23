// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// Gain factor for ECN CE mark ratio samples
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2290
const ECN_ALPHA_GAIN: f64 = 1.0 / 16.0;

// The maximum tolerated ratio of packets containing ECN CE markings
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2306
const ECN_THRESH: f64 = 0.5;

// On ECN CE markings, cut inflight_lo to (1 - ECN_FACTOR * ecn_alpha)
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2301
pub(super) const ECN_FACTOR: f64 = 0.33;

#[derive(Clone, Debug)]
pub(crate) struct State {
    /// The amount of explicit congestion experienced packets in the current round trip
    ce_count_in_round: u64,
    /// The amount of bytes delivered in the current round trip
    round_start_delivered_bytes: u64,
    /// Weighted average ratio of ECN CE marked packets with `ECN_ALPHA_GAIN` applied
    alpha: f64,
    /// True if the ECN CE count over the current round trip was too high
    ce_too_high: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ce_count_in_round: 0,
            round_start_delivered_bytes: 0,
            alpha: 1.0,
            ce_too_high: false,
        }
    }
}

impl State {
    /// Called on each new BBR round
    #[inline]
    pub(super) fn on_round_start(&mut self, delivered_bytes: u64, max_datagram_size: u16) {
        let delivered_bytes_in_round = delivered_bytes - self.round_start_delivered_bytes;
        // update alpha
        if delivered_bytes_in_round > 0 {
            let ce_ratio = ce_ratio(
                self.ce_count_in_round,
                delivered_bytes_in_round,
                max_datagram_size,
            );
            self.alpha = calculate_alpha(self.alpha, ce_ratio);
            self.ce_too_high = is_ce_too_high(ce_ratio);
        }

        self.round_start_delivered_bytes = delivered_bytes;
        self.ce_count_in_round = 0;
    }

    /// Called each time explicit congestion is recorded
    #[inline]
    pub(super) fn on_explicit_congestion(&mut self, ce_count: u64) {
        self.ce_count_in_round += ce_count;
    }

    /// Returns true if the ECN CE ratio over the latest round was too high
    #[inline]
    pub(super) fn is_ce_too_high_in_round(&self) -> bool {
        self.ce_too_high
    }

    /// Returns the ECN alpha value
    #[inline]
    pub(super) fn alpha(&self) -> f64 {
        self.alpha
    }
}

/// Calculate the ratio of ECN CE marked bytes to overall delivered bytes
#[inline]
pub(super) fn ce_ratio(ecn_ce_count: u64, delivered_bytes: u64, max_datagram_size: u16) -> f64 {
    // Estimate the number of bytes experiencing explicit congestion by multiplying
    // the ecn_ce_count by max_datagram_size
    let ecn_ce_bytes = ecn_ce_count.saturating_mul(max_datagram_size as u64) as f64;
    ecn_ce_bytes / delivered_bytes as f64
}

/// True if the given ECN `ce_ratio` exceeds the BBR ECN threshold
#[inline]
pub(super) fn is_ce_too_high(ce_ratio: f64) -> bool {
    ce_ratio > ECN_THRESH
}

/// Calculates the new ECN alpha value
///
/// Based on `bbr2_update_ecn_alpha` from the Linux TCP BBRv2 implementation
/// See https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L1392
#[inline]
fn calculate_alpha(alpha: f64, ce_ratio: f64) -> f64 {
    ((1.0 - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * ce_ratio).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{assert_delta, path::MINIMUM_MAX_DATAGRAM_SIZE, recovery::bbr::ecn};

    #[test]
    fn on_round_start() {
        let mut state = ecn::State::default();
        assert_delta!(1.0, state.alpha(), 0.0001);

        let delivered_bytes = 1000;
        state.on_round_start(delivered_bytes, MINIMUM_MAX_DATAGRAM_SIZE);

        // No ECN CE yet and alpha is currently at the initial value of 1,
        // so alpha is just 1 - alpha gain
        assert_delta!(1.0 - ECN_ALPHA_GAIN, state.alpha(), 0.0001);
        assert!(!state.is_ce_too_high_in_round());
        assert_eq!(delivered_bytes, state.round_start_delivered_bytes);

        state.on_explicit_congestion(10);
        assert_eq!(10, state.ce_count_in_round);

        let alpha = state.alpha();
        let prev_delivered = delivered_bytes;
        // 20 packets delivered, 10 of which were ECN CE marked
        let delivered_bytes = prev_delivered + 20 * MINIMUM_MAX_DATAGRAM_SIZE as u64;

        state.on_round_start(delivered_bytes, MINIMUM_MAX_DATAGRAM_SIZE);

        assert_delta!(
            (1.0 - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * 0.5,
            state.alpha(),
            0.0001
        );
        // ECN ce count is not above the 50% `ECN_THRESH`
        assert!(!state.is_ce_too_high_in_round());
        assert_eq!(delivered_bytes, state.round_start_delivered_bytes);
        // ce count is reset
        assert_eq!(0, state.ce_count_in_round);

        state.on_explicit_congestion(11);
        assert_eq!(11, state.ce_count_in_round);

        let alpha = state.alpha();
        let prev_delivered = delivered_bytes;
        let delivered_bytes = prev_delivered + 20 * MINIMUM_MAX_DATAGRAM_SIZE as u64;

        // 20 packets delivered, 11 of which were ECN CE marked
        state.on_round_start(delivered_bytes, MINIMUM_MAX_DATAGRAM_SIZE);

        assert_delta!(
            (1.0 - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * 11.0 / 20.0,
            state.alpha(),
            0.0001
        );
        // ECN ce count is above the 50% `ECN_THRESH`
        assert!(state.is_ce_too_high_in_round());
        assert_eq!(delivered_bytes, state.round_start_delivered_bytes);
        assert_eq!(0, state.ce_count_in_round);
    }
}
