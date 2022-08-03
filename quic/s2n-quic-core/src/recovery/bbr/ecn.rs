// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use num_rational::Ratio;
use num_traits::One;

// Gain factor for ECN CE mark ratio samples
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2290
const ECN_ALPHA_GAIN: Ratio<u64> = Ratio::new_raw(1, 16);

// The maximum tolerated ratio of packets containing ECN CE markings
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2306
const ECN_THRESH: Ratio<u64> = Ratio::new_raw(1, 2);

// On ECN CE markings, cut inflight_lo to (1 - ECN_FACTOR * ecn_alpha)
// Value from https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2301
pub(super) const ECN_FACTOR: Ratio<u64> = Ratio::new_raw(1, 3);

#[derive(Clone, Debug)]
pub(crate) struct State {
    /// The amount of explicit congestion experienced packets in the current round trip
    ce_count_in_round: u64,
    /// The amount of bytes delivered in the current round trip
    round_start_delivered_bytes: u64,
    /// Weighted average ratio of ECN CE marked packets with `ECN_ALPHA_GAIN` applied
    alpha: Ratio<u64>,
    /// True if the ECN CE count over the current round trip was too high
    ce_too_high: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ce_count_in_round: 0,
            round_start_delivered_bytes: 0,
            alpha: Ratio::one(),
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
    pub(super) fn alpha(&self) -> Ratio<u64> {
        self.alpha
    }
}

/// Calculate the ratio of ECN CE marked bytes to overall delivered bytes
#[inline]
pub(super) fn ce_ratio(
    ecn_ce_count: u64,
    delivered_bytes: u64,
    max_datagram_size: u16,
) -> Ratio<u64> {
    // Estimate the number of bytes experiencing explicit congestion by multiplying
    // the ecn_ce_count by max_datagram_size
    let ecn_ce_bytes = ecn_ce_count.saturating_mul(max_datagram_size as u64);
    Ratio::new(ecn_ce_bytes, delivered_bytes)
}

/// True if the given ECN `ce_ratio` exceeds the BBR ECN threshold
#[inline]
pub(super) fn is_ce_too_high(ce_ratio: Ratio<u64>) -> bool {
    ce_ratio > ECN_THRESH
}

#[inline]
fn calculate_alpha(alpha: Ratio<u64>, ce_ratio: Ratio<u64>) -> Ratio<u64> {
    ((Ratio::one() - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * ce_ratio).min(Ratio::one())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{path::MINIMUM_MTU, recovery::bbr::ecn};

    #[test]
    fn on_round_start() {
        let mut state = ecn::State::default();
        assert_eq!(Ratio::one(), state.alpha());

        let delivered_bytes = 1000;
        state.on_round_start(delivered_bytes, MINIMUM_MTU);

        // No ECN CE yet and alpha is currently at the initial value of 1,
        // so alpha is just 1 - alpha gain
        assert_eq!(Ratio::one() - ECN_ALPHA_GAIN, state.alpha());
        assert!(!state.is_ce_too_high_in_round());
        assert_eq!(delivered_bytes, state.round_start_delivered_bytes);

        state.on_explicit_congestion(10);
        assert_eq!(10, state.ce_count_in_round);

        let alpha = state.alpha();
        let prev_delivered = delivered_bytes;
        // 20 packets delivered, 10 of which were ECN CE marked
        let delivered_bytes = prev_delivered + 20 * MINIMUM_MTU as u64;

        state.on_round_start(delivered_bytes, MINIMUM_MTU);

        assert_eq!(
            (Ratio::one() - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * Ratio::new(1, 2),
            state.alpha()
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
        let delivered_bytes = prev_delivered + 20 * MINIMUM_MTU as u64;

        // 20 packets delivered, 11 of which were ECN CE marked
        state.on_round_start(delivered_bytes, MINIMUM_MTU);

        assert_eq!(
            (Ratio::one() - ECN_ALPHA_GAIN) * alpha + ECN_ALPHA_GAIN * Ratio::new(11, 20),
            state.alpha()
        );
        // ECN ce count is above the 50% `ECN_THRESH`
        assert!(state.is_ce_too_high_in_round());
        assert_eq!(delivered_bytes, state.round_start_delivered_bytes);
        assert_eq!(0, state.ce_count_in_round);
    }
}
