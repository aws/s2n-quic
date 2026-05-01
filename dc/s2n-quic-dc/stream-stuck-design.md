# Stream Stuck — Root Cause Analysis and Fix

## Symptom

In production, under high data load, streams hit the `StreamStuck` error (progress watchdog fires in `send/state.rs`). The sender is unable to make forward progress — it retransmits indefinitely but never receives ACKs.

## Reproduction

Two deterministic tests reproduce the issue:

- `stream_stuck_on_receiver_panic` — server drops the stream (simulating crash/cleanup) while client still has in-flight data
- `stream_stuck_on_sender_panic` — client drops while server is sending

## Root Cause Chain (proven with trace logs)

1. **Server drops stream** → CONNECTION_CLOSE sent, but lost due to packet loss
2. **Server enters TimeWait (5s) → Finished** → queue deallocated, credential erased from routing
3. **Client keeps probing** → packets arrive at server → "credential mismatch" → "unroutable"
4. **`unroutable_packets` task** generates FlowReset and sends it back to the client
5. **FlowReset dispatched to client's stream queue** (`send_stream queue_id=0`)
6. **Client recv worker processes FlowReset** → calls `recv::state::on_error()`
7. **`on_error()` silently returns** because recv state is `DataRead` (server sent empty FIN at t=4ms)
8. **FlowReset never reaches the send worker** → client sender stuck for 30s idle timeout

Two sub-problems:

- **Problem A**: FlowReset always routed to stream queue (recv worker), never reaches send worker
- **Problem B**: CONNECTION_CLOSE sent once, never retransmitted if lost

## Fix (Implemented)

### Fix 1: FlowReset Trigger-Based Routing (wire change)

Added a `trigger` field to the FlowReset packet indicating which packet type triggered it:

- `Trigger::Stream` (0) — triggered by unroutable stream packet → route to **control queue** (send worker)
- `Trigger::Control` (1) — triggered by unroutable control packet → route to **stream queue** (recv worker)

**Files changed:**

- `packet/secret_control/flow_reset.rs` — Added `Trigger` enum and field to `FlowReset` struct
- `stream/environment/udp.rs` — `unroutable_packets` sets trigger based on original packet type
- `socket/recv/router.rs` — `dispatch_flow_reset_packet` trait method passes trigger
- `stream/recv/dispatch.rs` — Routes FlowReset to correct queue based on trigger
- `socket/recv/router/zero_router.rs`, `with_map.rs` — Pass trigger through

This solves the original problem: FlowReset triggered by the client's stream data (which the server rejected) is now routed to the control queue, where the send worker processes it and terminates.

### Fix 2: CONNECTION_CLOSE Retransmission via PTO

Previously, CONNECTION_CLOSE was sent exactly once. If lost, the peer had to wait for TimeWait expiry + FlowReset (5+ seconds) or idle timeout (30+ seconds).

**Reset state machine** (`send/state/reset.rs`): Added throttled retransmission states:

- `Idle → Queued → Sent` (initial send)
- `Sent` + peer activity → `ThrottlingQueued` (immediate retransmit, then throttled)
- `ThrottlingQueued → Throttling` (after transmission, PTO-only retransmits)
- `Sent | Throttling` + PTO timeout → `Queued` (PTO re-queues with exponential backoff)

**Send state** (`send/state.rs`):

- `has_inflight_packets()` returns true when reset is `waiting_ack`
- `update_pto_timer()`/`force_arm_pto_timer()` work during reset state
- PTO timeout calls `reset.on_pto_timeout()` to re-queue CONNECTION_CLOSE

### Fix 3: Recv Worker Draining-State Packet Processing

Previously, `process_packets()` in the recv worker bailed early via `ensure!(self.shared.get_error().is_none())`. This prevented the recv worker from seeing incoming packets during TimeWait, so it could never retransmit CONNECTION_CLOSE in response to client probes.

**Recv worker** (`recv/worker.rs`): Removed the early-return guard so packets continue to be processed during draining/TimeWait states, allowing `on_new_packet_draining()` to trigger CONNECTION_CLOSE retransmission with throttled backoff.

## Test Results

All tests pass (no regressions):

- `stream_stuck_on_receiver_panic` — PASS (< 1s resolution)
- `stream_stuck_on_sender_panic` — PASS (< 1s resolution)
- `lost_flow_increase` — PASS
- `transmission_rate_fuzz` — PASS
- All other deterministic tests — PASS
- `fail_fast_unknown_path_secret` — pre-existing failure (not caused by these changes)
