# stream3 remaining work

This implementation is porting/cleaning up the stream2 POC. These are the remaining gaps to fully replace it.

## Critical (correctness)

### Test compilation errors (combinator/tests.rs)

- `Sender` trait → should be `UnboundedSender`
- `try_send_pick_two` is now an associated fn `PickTwo::try_send_pick_two`, not a free fn
- `TestItem` missing `StickyRoute::set_sender_id` impl

### Assembler `todo!()` — wheel interest after assembly (combinator.rs:448)

After `assemble::assemble` returns, the context needs to declare interest in timing wheels (tx wheel for next transmission, PTO wheel, idle wheel). The Assembler combinator needs fields for each wheel's sender, and should dispatch the context's wheel interest after yielding the assembled segments.

## High priority (observability)

### Queue depth visibility

Stream2 wraps inter-task channels with `GaugedQueue` for all key queues. Stream3 has none. Every sync channel boundary should be wrapped:

- `worker_batch_txs/rxs` (frame dispatch → send workers)
- `worker_ack_txs/rxs` (recv dispatch → send workers)
- `packet_tx/rx` (socket recv → packet dispatch)
- The unsync `socket_context_txs` (context resolver → per-socket assembler)
- `completed_tx/rx`, `cancelled_tx/rx` (completion paths)

This gives us `q.*` gauges equivalent to stream2.

## Medium priority (completeness)

### Missing ACK batching

Currently (once we fix the "no ACKs" bug) we'll send one ACK per received packet. The ACK wheel infrastructure (`AckWheelLinks`, `AckWheelAdapter`, `AckState`) exists for coalescing: schedule a short delay (~1ms), batch all packets received in that window into one ACK frame. Wire up an ACK wheel drain task in the recv worker.

### PTO probe not implemented (tasks.rs:390)

When the PTO wheel fires for a context, we need to generate a probe packet (retransmit one frame or send a PING-equivalent) to elicit an ACK from the peer.

### Idle context reclamation (tasks.rs:401)

When the idle wheel fires, we should clean up the `send::Context` (remove from cache, release CCA state, notify completion channels of remaining inflight frames as failed).

### Response frame routing (tasks.rs:562)

Response frames (ACKs generated on the recv path, FlowReset, FlowValidateRequest) currently go back through the shared `frame_tx` submission channel. This adds latency. Stream2 uses a dedicated channel + RetransmissionBatcher. Lower priority but worth tracking.

## Low priority (cleanup / future)

### Stale docs in tasks.rs

- `frame_dispatch` doc (line 47-108): accurate but mentions "Sticky routing and queue metrics are not yet implemented" — update after those are done
- `send_worker` doc (line 158-168): mentions "TODO: tx wheel (pacing/scheduling)" which now exists as the tx_wheel drain task
- `socket_recv_task` doc (line 482-489): "TODO: missing stream2 pipeline stages" section mentions recv metrics and recv-side pacing
- `packet_dispatch_task` doc (line 536-542): "TODO: missing stream2 pipeline stages" mentions dispatch counters (partially done) and queue depth

### recv.rs:136 — key rotation support

Currently only tracks the latest key. Packets with old key_ids after rotation fail to decrypt. Need a small cache of recent openers.

### recv.rs:224 — MTU-based ACK sizing

`generate_ack_packet` hardcodes `mtu = 1400` instead of using `path_secret_entry.max_datagram_size() - overhead`.

### ack.rs:140 — time-based loss detection

Only PN-threshold loss detection is implemented. Add `kTimeThreshold = 9/8 * max(smoothed_rtt, latest_rtt)` for time-based detection.

### writer.rs TODOs

- Auto-tune `max_inflight_bytes` based on completion queue delivery rate
- Pace frame transmissions at 1us interval (currently passing `None` for `transmission_time`)

### reader.rs TODOs

- Ordering guarantee around Reset messages in the queue
- Whether buffered (non-contiguous) reassembler data should contribute to MAX_DATA threshold
