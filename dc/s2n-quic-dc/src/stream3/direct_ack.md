# Direct ACK Channels

## Problem and Background

ACK frames currently flow through the same frame submission pipeline as data frames: recv dispatch generates an ACK, submits it to the sharded `frame_tx` channel, it passes through priority routing, batching, and PickTwo distribution before reaching a send worker. This adds latency to time-sensitive ACKs — delayed ACKs inflate the peer's RTT estimates, which directly impacts loss detection timing and congestion control responsiveness.

Additionally, the receiver currently generates an ACK in response to every ack-eliciting packet it receives. Under high throughput this consumes significant bandwidth with redundant acknowledgments. Most of that information is stale by the time it reaches the wire: packets arriving between ACK generation and ACK transmission aren't reflected.

The `ack_delay` field compounds the staleness problem. It's currently computed at encoding time on the recv dispatch worker, but what matters is the delay between receiving the largest acknowledged packet and the ACK actually hitting the wire. The gap between encoding and transmission is unaccounted for, making the peer's RTT samples noisier than necessary.

## Requirements

R1. ACK frames must bypass the shared `frame_tx` submission pipeline entirely.

R2. At most one ACK packet per recv state should be in the send pipeline at any time (ACK batching).

R3. The `ack_delay` field must reflect the actual delay at wire time, not at the moment the receiver decides to send.

R4. The receiver must learn when its ACK was transmitted so it can decide whether to send another (event-driven, non-blocking).

R5. If packets arrive between ACK encoding and transmission, the transmitted ACK should reflect the most recent state without requiring the receiver to actively re-submit.

R6. The design must not introduce contention between unrelated recv/send pairs.

## Goals

G1. Remove ACK frames from the shared frame pipeline so that data frame throughput and ACK latency are decoupled.

G2. Minimize work on the recv dispatch hot path — encoding and re-encoding ACK ranges should not block packet processing.

G3. Preserve the existing assembler's ability to coalesce ACK frames with data into the same packet (mixed packets reduce header overhead).

G4. Keep the sender's read-side critical section as short as possible — ideally a single Arc clone.

## Solution: NxM Direct Channels

Each (recv dispatch worker, send worker) pair gets a dedicated channel. Since recv dispatch workers already know which send worker owns a given sender_id (the mapping is deterministic via credential hashing), ACK transmissions route directly to the correct send worker without intermediation.

This mirrors the existing NxM pattern used for routing from the main scheduling worker to individual send workers. The channel is bounded (capacity 1 is sufficient given the at-most-one-in-flight invariant) and carries a lightweight handle to the shared ACK state rather than a full frame.

This satisfies R1 (bypasses frame_tx), R6 (no shared contention — each pair has its own channel), and G1 (ACKs are fully decoupled from the data path).

## Solution: Shared ACK State with RwLock and Versioning

Each recv state (keyed by `credentials::Id + remote_sender_id`) owns a shared ACK state behind a `RwLock`:

```
struct SharedAckState {
    body: Bytes,       // pre-encoded ACK ranges
    version: u64,      // monotonically increasing on each update
    largest_recv_time: Timestamp,  // when the largest PN was received
}
```

The receiver writes to this state whenever it decides to update (which may be deferred if the recv worker is saturated — natural backpressure). The write is: encode ranges into a `Bytes`, bump the version, update the timestamp. The lock hold is a pointer swap and two field writes.

The sender reads from this state at assembly time. The read is: clone the `Bytes` (Arc ref bump), copy the version and timestamp. The lock hold is three field copies.

The ACK transmission entry records which version it carried. When the completion notification arrives back at the recv worker, it compares `version_transmitted` against the current version. If stale, it immediately re-submits. If current, it idles until the next packet arrival triggers a new version.

This satisfies R2 (at-most-one in flight, gated by completion), R5 (sender always reads latest), G2 (recv can defer re-encoding), and G4 (read is just a Bytes clone).

## Solution: ACK Frame Header with ack_delay

Rather than encoding `ack_delay` into the body alongside ranges (as QUIC does), we lift it into the frame header. The existing `Header::Control` variant becomes an `Ack` variant:

```
Header::Ack {
    dest_sender_id: VarInt,
    ack_delay: VarInt,
    has_ecn: bool,
}
```

The body (payload) is exclusively the ACK ranges (and ECN counts if `has_ecn` is set). The sender computes `ack_delay` at assembly time as `now - largest_recv_time` from the shared state, then writes it into the header. This means the delay is stamped at the last possible moment before the packet hits the wire.

The tag byte already carries bits distinguishing frame types. We add an ECN-present flag so the sender knows whether ECN counts follow the ranges without inspecting the body.

This satisfies R3 (ack_delay at wire time) and keeps the body encoding simple — the receiver never needs to reason about the delay.

## Solution: Completion-Driven ACK Loop

A dedicated task per recv dispatch worker polls a completion queue. When an ACK transmission completes (the packet was handed to the socket), the entry returns carrying its recv state key and the version it transmitted.

The completion task:

1. Looks up the recv state by key.
2. Compares `version_transmitted` against the current `SharedAckState.version`.
3. If stale: re-reads the shared state, submits a new ACK transmission to the direct channel.
4. If current: marks the recv state as idle. The next inbound ack-eliciting packet will trigger a fresh cycle.

This is fully event-driven and non-blocking. The recv dispatch worker never blocks waiting for transmission — it fires and forgets. The completion task handles the re-send decision independently.

This satisfies R4 (event-driven notification) and R2 (at-most-one, since re-submission only happens after completion).

## Solution: Assembler Integration

The send worker's assembler gains a new phase between immediate-drain and pending-drain:

1. Drain immediate (existing — other control frames if any).
2. Check for a pending direct ACK: read shared state, compute ack_delay, encode the ACK header + body into the current packet. If there's room, continue to step 3 to coalesce with data.
3. Drain pending (data frames, subject to CWND).

ACK frames assembled via this path are not registered in the inflight map (same as today — ACKs are not ack-eliciting and not congestion-controlled). The packet they're coalesced into is registered if it also contains data.

After encoding, the assembler posts the completion entry (with key + version) to the completion channel for the recv worker.

This satisfies G3 (coalescing with data) and keeps the assembler as the single point of packet construction.

## Recommendations

**Use the Ack header variant over repurposing Control.** The Control variant was a stand-in. ACKs have distinct routing (direct channel), distinct metadata (ack_delay, ECN flag), and distinct assembly semantics. A dedicated variant makes these differences type-safe and eliminates runtime checks. (Supports R3, G3)

**Bound the direct channel at capacity 1.** The at-most-one-in-flight invariant means we never need more than one pending entry. A capacity of 1 provides natural backpressure: if the sender hasn't consumed the previous entry, the recv side knows an ACK is already queued and doesn't need to re-submit. (Supports R2, R6)

**Defer body re-encoding on the recv hot path.** The shared state update (write lock + Bytes swap) should happen only when the recv worker has budget. If it's saturated processing inbound packets, skip the re-encode — the completion loop will pick up whatever version is current. The ACK ranges only grow between updates, so staleness means "slightly less complete" not "wrong." (Supports G2)

**Keep the frame_tx path for non-ACK control frames.** FlowControl, FlowReset, etc. are rare and don't have the same latency sensitivity. Routing them through the existing pipeline avoids adding complexity to the direct channel. (Supports G1 by keeping the direct path focused)

**Version comparison uses simple inequality, not modular arithmetic.** A `u64` version counter won't wrap in practice (2^64 increments at 10Gpps would take ~58 years). This keeps the stale check trivial: `transmitted_version < current_version` means re-send. (Supports R5)

## Edge Cases

**Recv state expires while ACK is in flight**: The completion task looks up the recv state by key. If the state was evicted (idle timeout), the lookup fails and the completion is silently dropped. No harm — the peer's sender will eventually time out or receive an ACK from a subsequent connection.

**Send worker drains before reading shared state**: The direct channel entry is a lightweight handle (key + Arc to shared state). If the send worker picks it up but the recv state has since been cleared, the Bytes in the shared state are still valid (owned via Arc). The ACK goes out with the last-known ranges, which is correct.

**Multiple recv states routed to the same send worker**: Each has its own shared state and its own slot in the direct channel. The send worker processes them independently during assembly. No interaction between flows.

**Packet coalescing fills before ACK body fits**: If the assembler is mid-packet and the remaining space can't fit the ACK ranges, it finishes the current packet and starts a new one for the ACK. ACK-only packets are fine — they're small and don't count against CWND.
