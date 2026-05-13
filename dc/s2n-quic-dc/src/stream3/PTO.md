# PTO (Probe Timeout) Design

## Problem

When all inflight packets are lost and no ACKs arrive, the sender has no signal to trigger loss detection. PTO exists to break this deadlock: after a timeout, send something ack-eliciting so the peer generates an ACK, which then drives PN-threshold loss detection on old packets.

The PTO wheel infrastructure and state machine already exist in stream3. The wheel fires, hands us a context, but the handler is a no-op stub. We need to design what happens when it fires and how that integrates with the existing assembly pipeline.

## Additional Problem: Congestion Window Enforcement

CWND is currently not enforced (the check is `|| true` in `wheel_interest`). The reason it was disabled: ACK frames share the same pending queue as data frames, so gating on CWND blocks ACK delivery. Blocked ACKs starve the peer's loss detection, creating a deadlock.

PTO probes have the same property as ACKs here: they must bypass CWND to serve their purpose. This means fixing PTO properly also requires fixing the CWND enforcement story.

## Requirements

1. PTO must produce an ack-eliciting packet when it fires and there's unacked data in flight.
2. PTO probes must bypass CWND (RFC 9002 Section 6.2.4).
3. ACK frames must also bypass CWND (RFC 9002 Section 7: "packets containing only ACK frames do not count towards bytes in flight and are not congestion controlled").
4. When the original packet wasn't actually lost (delayed ACK arrives), we should not lose the RTT/CCA signal from it.
5. Successive PTO firings without ACKs should produce a chain of probes with exponential backoff (already handled by Pto state machine, capped at 16x).
6. PTO probe packets must skip at least one PN so the peer's ACK creates a gap that triggers PN-threshold loss detection on old packets.

## Goals

- Keep the PTO handler simple: minimal logic, delegates actual packet construction to the assembler.
- Don't duplicate assembly logic across the PTO handler and the existing assembler.
- Preserve RTT accuracy: when a probe is sent but the original was just delayed, the ACK for the original should still yield a correct RTT sample.
- Enable CWND enforcement for data frames without blocking ACKs or probes.

## Solution: Split Pending into Immediate/Pending

The Context currently has a single `pending: PendingFrames` queue. We split into two:

- `immediate`: frames that bypass CWND. Contains ACK frames (Control headers). Drained unconditionally by the assembler.
- `pending`: data frames, subject to CWND. Drained only when congestion window allows.

The split point is at batch insertion: when `push_batch` is called, frames with `Control` headers route to `immediate`, everything else to `pending`.

The assembler's drain order:

1. Drain `immediate` unconditionally (ACKs).
2. If a probe is owed, encode the oldest unacked data from inflight under a new PN (skipping a PN first).
3. Drain `pending` while CWND allows.

CCA interaction depends on what the packet ultimately contains:

- Pure ACK packets (only frames with `Control` header): do NOT call `on_packet_sent`, do not register in inflight. These are non-ack-eliciting, non-congestion-controlled.
- Any packet containing at least one non-ACK frame (whether from immediate, probe, or pending): DO call `on_packet_sent`, register in inflight. This includes mixed packets where ACKs are coalesced with data or probes.
- This distinction is already captured by the `is_ack_eliciting` flag in the assembler — the rule is simply: if ack-eliciting, register and call CCA.

## Solution: Probe Chains for RTT Preservation

When PTO fires and the assembler retransmits inflight data under a new PN, the old PN entry stays in the inflight map as a "shell" (frames removed, TransmissionInfo preserved). A forward pointer links old to new:

`inflight::Packet` gains `probed_to: Option<PacketNumber>`.

This creates a chain: PN_old -> PN_probe1 -> PN_probe2 (if multiple PTOs fire without any ACKs arriving). The chain is typically shallow since repeated PTOs with exponential backoff mean old packets are very likely lost.

ACK processing behavior on the chain:

- ACK arrives for a shell (has `probed_to`, empty frames): follow the chain to the end, complete the frames found there. RTT/CCA use the ACKed shell's `time_sent`, which is the accurate measurement. The probe entry stays in inflight with its own TransmissionInfo — if later ACKed it will be an empty shell (frames already completed) and gets cleared out; if swept by loss detection, `on_packet_lost` fires for CCA but there's nothing to retransmit.
- ACK arrives for the probe (the tail): complete its frames normally. PN-threshold loss detection sweeps the older shells, calls `on_packet_lost` with their cc_info.
- Loss detection hits a shell: call `on_packet_lost` as normal for CCA. Do NOT follow the chain or cancel the probe — it's still in flight and may succeed.

## Solution: Assembler-Driven Probing

Rather than having the PTO handler construct probe frames and insert them into a queue, the handler simply signals "send a probe" and the assembler does the work. This avoids duplicating assembly logic and keeps the PTO handler trivial.

The PTO handler:

1. `pto.on_timeout()` returns false (spurious, needs_update): recompute `wheel_interest`, dispatch, done.
2. Returns true: transition the probe state to "requested," schedule context for immediate TX.

The probe state is an enum on Context since we only send one probe per PTO. The assembler transitions it back after sending. The existing `Pto` struct already has machinery like `needs_update` that parallels this.

The assembler, after draining immediate and before draining pending, checks the probe state:

1. Find the oldest inflight packet with non-empty frames (skip shells).
2. Skip a PN (increment `next_packet_number` without using it) so the probe creates a gap.
3. Encode those frames into a new packet under the next PN.
4. Move the frames from old entry to new entry (old becomes a shell).
5. Set `old_entry.probed_to = new_pn`.
6. Transition probe state back to idle.

The frames never leave the inflight map — they transfer directly from old PN to new PN during assembly.

## Solution: Constant-Period Wheel Arming

The intrusive wheel doesn't support unlinking entries (removing would desync the list length). This creates a stale-target problem: if PTO backoff is 16x and an ACK resets it to 1x, the wheel entry is still armed at the 16x interval. We'd wait the full stale period before the handler fires and can reschedule.

Instead of arming the wheel at the backed-off interval, we always arm at the base PTO period (1x RTT multiplied by the initial backoff constant). The backoff is tracked purely as a fire counter: the PTO handler fires at the base rate and checks whether enough firings have elapsed to actually trigger a probe.

This means:

- Wheel target is always `last_sent_time + base_pto_period` — never scaled by backoff.
- Context tracks backoff as "firings remaining before probe." When the handler fires, decrement. If zero, probe. Otherwise re-arm.
- On ACK: reset firings remaining to the initial value (meaning the very next wheel firing is the real one — equivalent to resetting backoff to 1x).

This eliminates the stale-target problem entirely. After a backoff reset, the next wheel firing is at most one base period away. The cost is a few extra no-op wakeups during high backoff, but PTO is already a rare path and the handler is cheap (decrement + re-insert).

## Solution: Direct ACK Routing

Currently, ACK frames generated by recv dispatch workers go through the global `frame_tx` submission channel, then through priority routing, batching, and pick-two before reaching the send worker. This adds unnecessary latency for ACKs, which are time-sensitive (delayed ACKs inflate the peer's RTT estimates).

The recv dispatch workers already have a direct path to send workers via the `ack_rx` channel (used for incoming ACKs that drive loss detection). A similar direct channel for outbound ACK frames would bypass the shared submission pipeline entirely. The recv worker knows the destination credentials, and the send worker mapping is deterministic via credential hashing.

This is an optimization orthogonal to PTO but synergistic: with ACKs routed directly into `immediate` on the target context, the submission channel carries only data frames. This simplifies the CWND enforcement story since the shared pipeline never needs to reason about exempt frames.

## Edge Cases

**PTO fires but nothing is in flight**: If inflight drains between scheduling and firing (ACK arrived in the interim), `has_inflight()` check in the assembler means the probe is a no-op. The bumped backoff is harmless since `on_ack_received` already resets it.

**PTO fires while pending frames exist**: the pending data itself serves as the probe — it's ack-eliciting. The assembler will send it normally. No need to pull from inflight.

**Multi-hop chains**: successive PTO firings produce PN_0 -> PN_1 -> PN_2. When ACK for PN_0 arrives, follow probed_to to PN_2, complete frames there, remove PN_1 from inflight. When ACK for PN_2 arrives, complete frames normally, PN_0 and PN_1 get swept by PN-threshold loss.

**Mixed packets**: the assembler may fill remaining segment space with pending data alongside the probe. That's fine — the packet is registered in inflight as a unit. If lost, normal loss detection handles all constituent frames.

**Loss detection on a shell whose probe was already completed**: the shell's frames field is empty, so there's nothing to retransmit or complete. CCA still gets `on_packet_lost` for its sent_bytes, which is correct — those bytes were on the wire and lost.

## Implementation Order

1. Split pending into immediate/pending on Context.
2. Route Control frames to immediate during batch insertion.
3. Update assembler to drain immediate first, then pending with CWND check.
4. Re-enable CWND enforcement (remove `|| true`).
5. Add `probed_to: Option<PacketNumber>` to `inflight::Packet`.
6. Add probe state enum to Context.
7. Rework PTO arming to use constant-period wheel with fire counter for backoff.
8. Implement PTO handler: transition probe state, schedule immediate TX.
9. Implement probe assembly logic in assembler (including PN skip).
10. Update ACK processing to follow probe chains.
11. (Future) Direct ACK routing from recv workers to send worker contexts.
