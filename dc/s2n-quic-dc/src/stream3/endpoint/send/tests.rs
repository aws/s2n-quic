// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `Pto` and `ProbeState` logic.

use super::{ProbeState, Pto, INITIAL_PTO_BACKOFF};
use crate::clock::testing::Clock;
use core::time::Duration;
use s2n_quic_core::recovery::RttEstimator;

fn make_clock(millis: u64) -> Clock {
    Clock::new(Duration::from_millis(millis))
}

fn make_rtt(smoothed_millis: u64) -> RttEstimator {
    RttEstimator::new(Duration::from_millis(smoothed_millis))
}

// ── ProbeState transitions ────────────────────────────────────────────────────

#[test]
fn probe_state_initial_is_idle() {
    let state = ProbeState::default();
    assert!(!state.is_requested());
}

#[test]
fn probe_state_request_transitions_to_requested() {
    let mut state = ProbeState::default();
    state.request().unwrap();
    assert!(state.is_requested());
}

#[test]
fn probe_state_on_transmit_clears_request() {
    let mut state = ProbeState::default();
    state.request().unwrap();
    state.on_transmit().unwrap();
    assert!(!state.is_requested());
}

#[test]
fn probe_state_on_all_acked_clears_request() {
    let mut state = ProbeState::default();
    state.request().unwrap();
    state.on_all_acked().unwrap();
    assert!(!state.is_requested());
}

#[test]
fn probe_state_double_request_is_noop() {
    let mut state = ProbeState::default();
    state.request().unwrap();
    // Second request is a NoOp (already Requested) — should not panic
    let _ = state.request();
    assert!(state.is_requested());
}

#[test]
fn probe_state_on_transmit_from_idle_is_noop() {
    let mut state = ProbeState::default();
    // on_transmit from Idle is a NoOp — should not panic
    let _ = state.on_transmit();
    assert!(!state.is_requested());
}

// ── Pto::on_timeout backoff progression ──────────────────────────────────────

#[test]
fn pto_initial_state() {
    let pto = Pto::default();
    assert_eq!(pto.backoff, INITIAL_PTO_BACKOFF);
    assert_eq!(pto.firings_remaining, 0);
    assert!(!pto.needs_update);
    assert!(pto.arm_base.is_none());
    assert!(pto.last_sent_time.is_none());
}

#[test]
fn pto_first_timeout_fires_probe() {
    let mut pto = Pto::default();
    assert!(pto.on_timeout(), "first timeout should signal a probe");
    // After doubling: backoff = INITIAL*2 = 2, firings_remaining = 2-1 = 1
    assert_eq!(pto.backoff, 2);
    assert_eq!(pto.firings_remaining, 1);
}

#[test]
fn pto_second_timeout_is_countdown() {
    let mut pto = Pto::default();
    pto.on_timeout(); // fires probe, sets firings_remaining=1
    assert!(
        !pto.on_timeout(),
        "second timeout is a countdown firing, not a probe"
    );
    assert_eq!(pto.firings_remaining, 0);
}

#[test]
fn pto_third_timeout_fires_probe_again() {
    let mut pto = Pto::default();
    pto.on_timeout(); // 1st: probe, backoff→2, firings=1
    pto.on_timeout(); // 2nd: countdown, firings→0
    assert!(pto.on_timeout(), "3rd timeout should fire probe");
    // backoff → 4, firings_remaining → 3
    assert_eq!(pto.backoff, 4);
    assert_eq!(pto.firings_remaining, 3);
}

#[test]
fn pto_backoff_caps_at_sixteen() {
    let mut pto = Pto::default();
    // Drive backoff to max by firing probes many times.
    for _ in 0..200 {
        pto.on_timeout();
        if pto.backoff == 16 {
            break;
        }
    }
    assert_eq!(pto.backoff, 16, "backoff should cap at 16");

    // Confirm it stays at 16
    for _ in 0..20 {
        if pto.on_timeout() {
            assert_eq!(pto.backoff, 16, "backoff should remain capped at 16");
        }
    }
}

// ── Pto::needs_update path ────────────────────────────────────────────────────

#[test]
fn pto_on_packet_sent_sets_needs_update() {
    let clock = make_clock(100);
    let now = clock.get_time();
    let mut pto = Pto::default();
    pto.on_packet_sent(now);
    assert!(pto.needs_update, "on_packet_sent should set needs_update");
    assert!(pto.last_sent_time.is_some());
    assert!(pto.arm_base.is_none(), "on_packet_sent resets arm_base");
}

#[test]
fn pto_needs_update_suppresses_probe() {
    let clock = make_clock(100);
    let now = clock.get_time();
    let mut pto = Pto::default();
    pto.on_packet_sent(now);
    // on_timeout while needs_update is set should NOT fire a probe
    assert!(
        !pto.on_timeout(),
        "needs_update path should not fire a probe"
    );
    assert!(!pto.needs_update, "needs_update cleared after timeout");
    assert!(pto.arm_base.is_none(), "arm_base reset in needs_update path");
}

#[test]
fn pto_needs_update_then_probe_fires() {
    let clock = make_clock(100);
    let now = clock.get_time();
    let mut pto = Pto::default();
    pto.on_packet_sent(now);
    pto.on_timeout(); // clears needs_update, no probe
    assert!(
        pto.on_timeout(),
        "after clearing needs_update, probe should fire"
    );
}

// ── Pto::on_ack_received resets state ────────────────────────────────────────

#[test]
fn pto_on_ack_resets_backoff() {
    let mut pto = Pto::default();
    pto.on_timeout(); // probe, backoff → 2
    pto.on_timeout(); // countdown
    pto.on_timeout(); // probe, backoff → 4
    assert_eq!(pto.backoff, 4);

    pto.on_ack_received(true);
    assert_eq!(
        pto.backoff, INITIAL_PTO_BACKOFF,
        "ACK should reset backoff"
    );
    assert_eq!(pto.firings_remaining, 0, "ACK should reset countdown");
    assert!(pto.arm_base.is_none(), "ACK should reset arm_base");
}

#[test]
fn pto_on_ack_with_no_remaining_inflight_clears_probe_state() {
    let mut pto = Pto::default();
    pto.probe_state.request().unwrap();
    assert!(pto.probe_state.is_requested());

    pto.on_ack_received(false);
    assert!(
        !pto.probe_state.is_requested(),
        "probe_state should be cleared when all inflight ACKed"
    );
}

#[test]
fn pto_on_ack_with_remaining_inflight_keeps_probe_state() {
    let mut pto = Pto::default();
    pto.probe_state.request().unwrap();

    pto.on_ack_received(true);
    assert!(
        pto.probe_state.is_requested(),
        "probe_state should remain Requested when inflight remains"
    );
}

#[test]
fn pto_on_ack_with_remaining_inflight_sets_needs_update() {
    let mut pto = Pto::default();
    pto.on_ack_received(true);
    assert!(
        pto.needs_update,
        "needs_update should be set when inflight remains after ACK"
    );
}

#[test]
fn pto_on_ack_with_no_inflight_clears_needs_update() {
    let mut pto = Pto::default();
    pto.on_ack_received(false);
    assert!(
        !pto.needs_update,
        "needs_update should be cleared when all inflight ACKed"
    );
}

// ── Pto::is_armed ─────────────────────────────────────────────────────────────

#[test]
fn pto_not_armed_initially() {
    let pto = Pto::default();
    assert!(!pto.is_armed(), "fresh Pto should not be armed");
}

#[test]
fn pto_armed_after_packet_sent() {
    let clock = make_clock(100);
    let mut pto = Pto::default();
    pto.on_packet_sent(clock.get_time());
    assert!(pto.is_armed(), "Pto should be armed after packet sent");
}

#[test]
fn pto_armed_after_next_target_called() {
    let clock = make_clock(100);
    let rtt = make_rtt(10);
    let mut pto = Pto::default();
    pto.on_packet_sent(clock.get_time());
    let target = pto.next_target(&clock, &rtt);
    assert!(target.is_some(), "next_target should return a timestamp");
    assert!(pto.is_armed(), "Pto armed after next_target sets arm_base");
}

// ── Pto::next_target timing ───────────────────────────────────────────────────

#[test]
fn pto_next_target_advances_arm_base() {
    let clock = make_clock(100);
    let rtt = make_rtt(20);
    let mut pto = Pto::default();
    pto.on_packet_sent(clock.get_time());

    let t1 = pto.next_target(&clock, &rtt).unwrap();
    let t2 = pto.next_target(&clock, &rtt).unwrap();
    assert!(
        t2 > t1,
        "successive next_target calls should advance arm_base"
    );
}

#[test]
fn pto_on_packet_sent_resets_arm_base() {
    let clock = make_clock(100);
    let rtt = make_rtt(20);
    let mut pto = Pto::default();
    pto.on_packet_sent(clock.get_time());
    pto.next_target(&clock, &rtt); // sets arm_base

    clock.advance(Duration::from_millis(50));
    pto.on_packet_sent(clock.get_time());
    assert!(pto.arm_base.is_none(), "on_packet_sent resets arm_base");
    assert!(pto.needs_update);
}

#[test]
fn pto_backoff_sequence_matches_expected_probe_count() {
    // Verify the exact sequence of probes and countdowns.
    // Backoff starts at 1. After a probe, it doubles.
    // firings_remaining = new_backoff - 1 countdowns before next probe.
    //
    // Sequence (backoff 1→2→4→8):
    //   timeout 1: probe  (backoff→2, remaining→1)
    //   timeout 2: count  (remaining→0)
    //   timeout 3: probe  (backoff→4, remaining→3)
    //   timeouts 4,5,6: count
    //   timeout 7: probe  (backoff→8, remaining→7)
    let mut pto = Pto::default();
    let expected = [
        true, false, // backoff 2
        true, false, false, false, // backoff 4
        true, false, false, false, false, false, false, false, // backoff 8
    ];
    for (i, &should_probe) in expected.iter().enumerate() {
        let result = pto.on_timeout();
        assert_eq!(
            result, should_probe,
            "timeout #{}: expected probe={should_probe} but got {result}",
            i + 1
        );
    }
}
