// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for send-side queue accounting and PTO / `ProbeState` logic.

use super::{Context, PendingFrames, ProbeState, Pto, INITIAL_PTO_BACKOFF};
use crate::{
    byte_vec::ByteVec,
    clock::testing::Clock,
    counter::Registry,
    packet::datagram::QueuePair,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::ByteCost,
    stream3::{
        endpoint::combinator::FrameBatch,
        frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
    },
};
use bytes::Bytes;
use core::time::Duration;
use s2n_quic_core::recovery::RttEstimator;
use std::sync::Arc;

fn make_clock(millis: u64) -> Clock {
    Clock::new(Duration::from_millis(millis))
}

fn make_rtt(smoothed_millis: u64) -> RttEstimator {
    RttEstimator::new(Duration::from_millis(smoothed_millis))
}

fn make_pending_frames() -> PendingFrames {
    let registry = Registry::default();
    PendingFrames::new(registry.register_queue_gauge("test.pending"))
}

fn make_path_secret_entry() -> Arc<PathSecretEntry> {
    PathSecretEntry::fake("127.0.0.1:9999".parse().unwrap(), None)
}

fn make_frame(payload_len: usize) -> crate::intrusive_queue::Entry<Frame> {
    let mut payload = ByteVec::new();
    if payload_len > 0 {
        payload.push_back(Bytes::from(vec![0; payload_len]));
    }

    Frame {
        header: Header::FlowData {
            queue_pair: QueuePair {
                source_queue_id: s2n_quic_core::varint::VarInt::from_u8(1),
                dest_queue_id: s2n_quic_core::varint::VarInt::from_u8(2),
            },
            stream_id: s2n_quic_core::varint::VarInt::from_u8(3),
            offset: s2n_quic_core::varint::VarInt::ZERO,
            is_fin: false,
        },
        source_sender_id: s2n_quic_core::varint::VarInt::MAX,
        payload,
        path_secret_entry: make_path_secret_entry(),
        completion: None,
        status: TransmissionStatus::Pending,
        ttl: DEFAULT_TTL,
        transmission_time: None,
    }
    .into()
}

#[test]
fn pending_frames_tracks_byte_cost_through_push_pop_and_requeue() {
    let mut pending = make_pending_frames();
    let frame_a = make_frame(8);
    let cost_a = frame_a.byte_cost() as usize;
    let frame_b = make_frame(32);
    let cost_b = frame_b.byte_cost() as usize;

    pending.push_back(frame_a);
    pending.push_back(frame_b);
    assert_eq!(pending.len(), 2);
    assert_eq!(pending.byte_cost(), cost_a + cost_b);

    let popped = pending.pop_front().expect("frame should be present");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.byte_cost(), cost_b);

    pending.push_front(popped);
    assert_eq!(pending.len(), 2);
    assert_eq!(pending.byte_cost(), cost_a + cost_b);

    let first = pending.pop_front().expect("first frame should be present");
    let second = pending.pop_front().expect("second frame should be present");
    assert_eq!(first.byte_cost() as usize, cost_a);
    assert_eq!(second.byte_cost() as usize, cost_b);
    assert!(pending.is_empty());
    assert_eq!(pending.byte_cost(), 0);
}

#[test]
fn pending_frames_append_queue_uses_supplied_batch_cost_exactly() {
    let mut pending = make_pending_frames();
    let frame_a = make_frame(5);
    let cost_a = frame_a.byte_cost();
    let frame_b = make_frame(11);
    let cost_b = frame_b.byte_cost();

    let mut queue = crate::intrusive_queue::Queue::new();
    queue.push_back(frame_a);
    queue.push_back(frame_b);

    let batch_cost = cost_a + cost_b;
    pending.append_queue(queue, batch_cost);

    assert_eq!(pending.len(), 2);
    assert_eq!(pending.byte_cost(), batch_cost as usize);

    let drained_cost = pending
        .pop_front()
        .into_iter()
        .chain(pending.pop_front())
        .map(|frame| frame.byte_cost() as usize)
        .sum::<usize>();
    assert_eq!(drained_cost, batch_cost as usize);
    assert!(pending.is_empty());
    assert_eq!(pending.byte_cost(), 0);
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
    assert!(
        pto.arm_base.is_none(),
        "arm_base reset in needs_update path"
    );
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
    assert_eq!(pto.backoff, INITIAL_PTO_BACKOFF, "ACK should reset backoff");
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
            result,
            should_probe,
            "timeout #{}: expected probe={should_probe} but got {result}",
            i + 1
        );
    }
}

// ── publish_sender_load_score tests ──────────────────────────────────────────

/// Build a `Context` backed by an entry that has `sender_count` pre-allocated sender slots.
///
/// `peer_data_addrs` is populated so that `Context::new` can resolve the destination address.
fn make_context_with_sender_slots(
    sender_count: usize,
    registry: &Registry,
) -> (Context, Arc<PathSecretEntry>) {
    let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();
    let entry = PathSecretEntry::fake_with_socket_senders(peer, None, sender_count);
    entry.set_peer_data_addrs(&[peer]);
    let ctx = Context::new(
        &entry,
        registry.register_queue_gauge("test.inflight"),
        registry.register_queue_gauge("test.ack"),
        registry.register_queue_gauge("test.pending"),
        0,
    )
    .expect("Context::new should succeed with peer_data_addrs populated");
    (ctx, entry)
}

/// `push_batch` must call `publish_sender_load_score` so that pick-two sees an up-to-date
/// backlog immediately after frames are enqueued — not only after the next send or ACK.
///
/// The test verifies that the score includes the enqueued frame's drain delay, not merely
/// that a timestamp was written.  It does so by comparing against an empty-queue baseline
/// published at the same instant: if the drain delay were missing, the two scores would
/// be equal.
#[test]
fn push_batch_immediately_refreshes_sender_load_score() {
    let registry = Registry::default();
    let (mut ctx, entry) = make_context_with_sender_slots(1, &registry);
    let clock = make_clock(1000);
    let now: s2n_quic_core::time::Timestamp = clock.get_time().into();

    // Establish a baseline: score with no queued frames at this instant.
    ctx.publish_sender_load_score(now);
    let score_empty_queue = entry.sender_load_score(0);

    // Enqueue a frame with a non-trivial payload.
    let frame = make_frame(512);
    let batch = FrameBatch::single(frame);
    let _ = ctx.push_batch(batch, &clock);

    // push_batch must have refreshed the score and the new score must be strictly
    // higher than the empty-queue baseline — the difference is the drain delay for
    // the enqueued bytes.
    let score_with_frame = entry.sender_load_score(0);
    assert!(
        score_with_frame > score_empty_queue,
        "push_batch should include the enqueued frame's drain delay in the score; \
         empty_queue={score_empty_queue}, with_frame={score_with_frame}"
    );
}

/// A sender whose CCA is cwnd-limited (bytes_in_flight ≥ cwnd) should receive a congestion
/// penalty of one smoothed RTT, making it look more loaded than an uncongested sender with
/// the same queued bytes and the same wall-clock time.
#[test]
fn cwnd_limited_adds_rtt_penalty_to_load_score() {
    let registry = Registry::default();
    let (mut ctx_congested, entry_congested) = make_context_with_sender_slots(1, &registry);
    let (ctx_idle, entry_idle) = make_context_with_sender_slots(1, &registry);

    let clock = make_clock(1000);
    let now: s2n_quic_core::time::Timestamp = clock.get_time().into();

    // Fill up ctx_congested's inflight past cwnd to trigger is_congestion_limited().
    // Send two packets that each exceed half the current cwnd.
    let rtt_clone = ctx_congested.rtt_estimator.clone();
    let cwnd = ctx_congested.cca.congestion_window();
    let pkt_size = ((cwnd / 2).saturating_add(1)).clamp(1, u16::MAX as u32) as u16;
    let _ = ctx_congested.cca.on_packet_sent(now, pkt_size, true, &rtt_clone);
    let _ = ctx_congested.cca.on_packet_sent(now, pkt_size, true, &rtt_clone);

    assert!(
        ctx_congested.cca.is_congestion_limited(),
        "ctx_congested should be cwnd-limited after filling inflight beyond cwnd={cwnd}"
    );

    // Publish with identical 'now' and zero queued bytes so the only difference is the penalty.
    ctx_congested.publish_sender_load_score(now);
    ctx_idle.publish_sender_load_score(now);

    let score_congested = entry_congested.sender_load_score(0);
    let score_idle = entry_idle.sender_load_score(0);

    assert!(
        score_congested > score_idle,
        "cwnd-limited sender (score={score_congested}) should have a higher load score \
         than an uncongested sender (score={score_idle})"
    );

    // The gap must be at least one full smoothed RTT (the congestion penalty is exactly
    // one smoothed RTT).  Timestamps are microsecond-granular so allow up to 1 µs of
    // rounding error in the stored score.
    let srtt_ns = ctx_congested.rtt_estimator.smoothed_rtt().as_nanos() as u64;
    let delta = score_congested - score_idle;
    assert!(
        delta + 1000 >= srtt_ns,
        "congestion penalty delta ({delta} ns) should be ≈ one full smoothed_rtt ({srtt_ns} ns)"
    );
}

/// When BBR's pacing gate is active (`earliest_departure_time` > `now`), the load score should
/// use EDT as its base rather than `now`, so pacing-gated senders appear more loaded than idle
/// senders at the same wall-clock time.
///
/// BBR pacing works in bursts: it sets EDT=now on the first send of a new burst (initialising
/// the departure time) and advances EDT by `send_quantum / pacing_rate` on each subsequent
/// burst.  Two back-to-back sends therefore leave EDT strictly in the future.
#[test]
fn edt_floor_raises_score_when_pacing_gated() {
    let registry = Registry::default();
    let (mut ctx_paced, entry_paced) = make_context_with_sender_slots(1, &registry);
    let (ctx_idle, entry_idle) = make_context_with_sender_slots(1, &registry);

    let clock = make_clock(1000);
    let t0: s2n_quic_core::time::Timestamp = clock.get_time().into();

    // Two back-to-back sends of a full quantum push EDT strictly into the future:
    //   send #1 – BBR initialises next_packet_departure_time = t0 + INITIAL_INTERVAL (= t0)
    //   send #2 – BBR advances EDT = max(EDT, t0) + interval = t0 + interval > t0
    let rtt_clone = ctx_paced.rtt_estimator.clone();
    let quantum = ctx_paced.cca.send_quantum().min(u16::MAX as usize) as u16;
    let _ = ctx_paced.cca.on_packet_sent(t0, quantum, true, &rtt_clone);
    let _ = ctx_paced.cca.on_packet_sent(t0, quantum, true, &rtt_clone);

    let edt = ctx_paced
        .cca
        .earliest_departure_time()
        .expect("BBR should set earliest_departure_time after two on_packet_sent calls");
    assert!(
        edt > t0,
        "EDT should be strictly after t0; two sends should have advanced the pacing window"
    );

    // Choose `now` that is after t0 but still strictly before EDT so the floor kicks in.
    // Use 1 µs — the minimum Timestamp increment — rather than 1 ns (which would be
    // rounded back to t0 due to Timestamp's microsecond granularity).
    let now_before_edt = t0 + Duration::from_micros(1);
    assert!(
        now_before_edt < edt,
        "test setup: now_before_edt must be < edt for the floor to apply"
    );

    // Publish scores from both contexts at the same instant.
    ctx_paced.publish_sender_load_score(now_before_edt);
    ctx_idle.publish_sender_load_score(now_before_edt);

    let score_paced = entry_paced.sender_load_score(0);
    let score_idle = entry_idle.sender_load_score(0);

    // The pacing-gated sender must not look cheaper than the idle sender.
    assert!(
        score_paced >= score_idle,
        "pacing-gated sender (score={score_paced}) should appear at least as loaded as \
         the idle sender (score={score_idle})"
    );

    // The paced score must be rooted at EDT, so it must be ≥ EDT expressed in nanoseconds.
    let edt_ns = unsafe { edt.as_duration().as_nanos() as u64 };
    assert!(
        score_paced >= edt_ns,
        "paced score ({score_paced}) should be ≥ EDT nanoseconds ({edt_ns})"
    );
}
