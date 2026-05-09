# Frame Aggregation Design

## Context and Motivation

The current stream2 architecture maintains a strict one-to-one-to-one mapping: one PartialDatagram becomes one GSO segment becomes one packet number becomes one entry in the packet number map. The Writer fragments application data into MTU-sized chunks, wraps each in a PartialDatagram with full routing info, groups them into GSO batches, and submits them to the wheel. The encoder assigns a packet number to each, encrypts each individually, and the socket sends them as a GSO datagram.

This works beautifully for bulk transfers. A single stream fills entire GSO batches with maximum-MTU segments, so per-packet overhead is negligible relative to payload throughput. A single stream doing a bulk transfer saturates 25 Gbps.

The problem appears with high-fan-out small-payload workloads. With 500k streams each sending roughly 1KB RPCs, every single RPC becomes its own packet number. You pay the full per-packet cost on both sides: one encryption and auth tag per packet, one packet_number_map insertion, one individual ACK generation, one ACK processing pass, one completion notification dispatch. The payloads are tiny but the control plane work scales linearly with packet count. The result is roughly 1 Gbps out of a 25 Gbps pipe. The bottleneck is per-packet overhead, not bandwidth.

The fundamental fix is to decouple the application's unit of work from the transport's unit of work. Multiple frames from different streams should pack into a single encrypted packet, sharing one packet number, one encryption pass, and one ACK.

## The Core Abstraction Shift

Today the data path looks like:

Writer produces PartialDatagram, groups into Batch, submits to Wheel, Wheel yields to PathResolver which attaches context, Encoder encrypts each segment with its own packet number, Socket sends the GSO datagram.

The proposed path:

Writer produces Frames, submits to Wheel, Wheel groups by peer and yields to Socket Worker, Socket Worker pushes into Peer Context queue, Peer Context self-schedules via local wheel, assembles multiple Frames into packets, encrypts each packet once, Socket sends the GSO datagram.

The Frame becomes the application's unit of work. The packet becomes the transport's unit of work. They decouple. For small payloads, many frames pack into a single packet. For large payloads, a single frame fills an entire packet by itself. The architecture adapts naturally to both extremes.

## Frame Definition

A Frame is the universal unit of work submitted by application-level components (Writers, control message senders, reset initiators). Every variant that exists today in RoutingInfo becomes a Frame type. FlowInit, FlowInitValidate, FlowValidateRequest, FlowData, FlowControl, FlowReset — all are Frames.

Each Frame carries its routing metadata (the same fields as today's RoutingInfo variants, including the full queue pair since frames in a single packet can target entirely different queues on the receiver), a payload (which may be empty for control frames or resets), a completion sender for ACK notification, a TTL counter for retransmission limits, and a should_transmit flag for cancellation.

The Frame does not know or care about GSO, batch sizes, segment uniformity, or packet numbers. It is purely an application-level concept. The transport layer decides how to pack, encrypt, and transmit Frames.

## Writer Changes

The Writer no longer builds GSO batches or interacts with the batch::Builder. Its responsibility narrows further: fragment application data into payload-sized chunks, wrap each in a Frame, and submit them.

The Writer needs to know the maximum payload size for a Frame so it can fragment correctly. This comes from the path secret entry's max_datagram_size (the raw UDP payload capacity) minus a per-frame header budget. The header budget is a hybrid estimate: worst-case varint sizes for fields the Writer doesn't know at submission time (like source_sender_id, which gets filled in later, and the overall datagram-level header including credentials) plus actual varint-encoded sizes for fields the Writer does know (stream_id, offset, queue_ids). This is similar to the current MAX_FLOW_DATA_HEADER_OVERHEAD calculation but more precise for the fields with known values.

The Writer associates a target transmission time with each Frame. To prevent a single stream from monopolizing a burst window, the Writer caps total payload at roughly 64KB per target time slot. When the cap is hit, it advances to the next slot at 1 microsecond granularity. These target times are advisory — the actual pacing happens downstream at the instance pacer and peer context level — but they ensure that a prolific stream's frames interleave fairly with frames from other streams rather than forming a single massive burst.

The Writer submits an intrusive queue of Frames to the wheel. Flow control logic (local inflight_bytes tracking, remote MAX_DATA budget, completion polling) remains unchanged. The Writer still tracks per-frame bytes and frees them when completions arrive.

## Instance Wheel and Pacer

The wheel receives Frame queues from Writers and is responsible for temporal interleaving across all streams in the process. When it drains due frames, it groups them by destination peer. Peer identity is determined by path_secret_entry pointer comparison — frames sharing the same Arc<PathSecretEntry> are destined for the same peer and can be aggregated.

The wheel accumulates up to roughly 64KB of frames per peer per drain cycle, then submits that per-peer intrusive queue to the socket worker. This 64KB limit corresponds to the maximum GSO datagram size and ensures no single peer monopolizes a drain cycle.

During merging, the wheel respects sender_id stickiness constraints. Frames with requires_sticky_sender_id (FlowInit, FlowInitValidate, FlowValidateRequest) must be routed to their designated sender socket, same as today's batch builder enforces. Non-sticky frames can be freely distributed.

Retransmission frames are prioritized above regular transmissions. When the wheel has both retransmissions and fresh frames ready at the same time slot, retransmissions drain first. This ensures that lost data gets another chance before new data consumes capacity.

## Socket Worker and Peer Context

This is where the architecture diverges most from today.

The socket worker receives a per-peer Frame queue from the wheel and immediately looks up the Peer Context for that peer. It pushes the frames into the Peer Context's internal queue. If the Peer Context is not already registered in the local timing wheel, it registers itself for the next transmission time that the congestion controller allows.

The Peer Context is now an active participant rather than a passive attachment. It owns its frame queue, manages its own scheduling, and performs packet assembly. The PathResolver and Encoder stages from today's pipeline are absorbed into the Peer Context.

When the Peer Context fires from the local wheel, it performs packet assembly:

First, it drains frames from its queue, enough to fill a GSO datagram (just under 64KB total across all segments). Each segment is MTU-sized and can contain multiple frames. A bunch of 100-byte payloads from different streams all fit in a single segment. Large payloads naturally fill segments by themselves.

Second, it writes the per-frame metadata into the packet header region. Each frame's metadata tag and routing fields are serialized contiguously. The payload bytes for all frames in that segment follow the metadata block.

Third, it encrypts each segment as a single unit. One packet number, one encryption pass, one auth tag covering all frame metadata and payloads in that segment. This is where the per-packet cost reduction materializes — instead of encrypting each small payload individually, a single encryption covers many.

Fourth, it sends the assembled GSO datagram via sendmsg, same as today.

Fifth, it registers the transmission in the packet number map. Each entry stores the list of Frames associated with that packet number, replacing the current single-PartialDatagram-per-entry model.

If the Peer Context still has frames queued after assembly (because the congestion window was smaller than the queue depth, or because frames arrived during assembly), it re-registers itself in the local wheel for the next CCA-permitted slot. This creates a natural drain loop gated by congestion control.

## On-Wire Packet Format

The packet format within a single segment (one packet number):

The packet-level header comes first. This contains the same fields as today's datagram header: tag byte, credentials ID, key ID, wire version, source control port, packet number. These are shared across all frames in the segment.

Following the packet header is the frame metadata region. Each frame is encoded as a type tag followed by its routing fields. FlowData has source_sender_id, queue_pair, stream_id, offset. FlowInit has source_sender_id, source_queue_id, dest_acceptor_id, attempt_id, stream_id. FlowControl has source_sender_id, queue_pair, stream_id. And so on — the same variants and fields as today's RoutingInfo enum. The type tags use the same discriminant values.

After the metadata region comes the payload region. Frame payloads are concatenated in the same order as their metadata entries. The payload length for each frame is derivable from the metadata (FlowData carries an implicit length from the offset progression, or we encode it explicitly — to be determined during implementation).

The auth tag closes the segment, covering everything from the packet header through the end of the payload region.

The packet-level header already encodes the total header length and total payload length, so the receiver knows where metadata ends and payloads begin without needing an explicit frame count. The decoder iterates metadata tags until it exhausts the header region, then maps payloads sequentially.

## Receiver Changes

The receiver decrypts a segment as one unit (same cost as today for a single-frame packet, but now amortized across multiple frames). After decryption, it iterates over the frame metadata entries in the header region.

For each frame, the receiver extracts the routing info and dispatches the corresponding payload slice to the appropriate handler. FlowData goes to the stream's reassembly queue. FlowInit goes to the acceptor registry. FlowControl goes to the stream's control channel. FlowReset triggers the appropriate reset logic. This is the same dispatch logic as today, just executed multiple times per packet instead of once.

A single decryption operation now services multiple application-level deliveries. For the 500k small-stream workload, this means the receiver's decryption cost drops by the same factor as the sender's encryption cost.

## ACK Processing and Completion Flow

ACKs still reference packet number ranges, same as today. The ACK format is unchanged. What changes is the fan-out when processing an ACK.

When a packet number is ACKed, the packet_number_map entry yields a list of Frames rather than a single PartialDatagram. Each Frame in the list gets its completion notification fired (success). The Writer frees inflight_bytes for each completed Frame independently. A single ACK range can retire dozens of small-stream Frames simultaneously.

This drastically reduces ACK traffic for small-payload workloads. Instead of one ACK per stream write, one ACK covers an entire packet's worth of aggregated frames. The ACK rate scales with packet count, not frame count.

Congestion control accounting uses the total bytes sent on the wire per packet (all frame payloads plus metadata overhead). The CCA sees aggregate bytes, not per-frame bytes. on_packet_sent and on_packet_acked fire once per packet number with the total segment byte count.

## Loss Detection and Retransmission

When a packet is declared lost, the packet_number_map entry yields its list of Frames. Each Frame is processed individually:

If the Frame's should_transmit flag is false (the stream was cancelled or the Writer was dropped), the Frame is dropped silently. Its completion fires with a cancellation status.

If the Frame's TTL has reached zero, it completes with a failure status. The Writer receives this as a transmission error.

Otherwise, the TTL is decremented and the Frame is re-queued into the instance wheel at retransmission priority. It goes through the entire aggregation pipeline again — it may end up packed into a completely different packet with different companion frames. The original packing was opportunistic; retransmission packing is equally opportunistic.

Retransmission frames burst at the same 64KB per 1 microsecond cadence as fresh frames, ensuring they interleave with other retransmissions rather than forming a single large burst. Their priority ensures they drain before fresh data when both are ready at the same time slot.

The TTL mechanism provides a bounded retransmission count. Rather than retransmitting indefinitely and relying on an idle timeout to eventually declare the peer dead, each frame has a finite number of chances. When TTL hits zero, the frame fails immediately and the Writer learns about it through the completion channel. This gives the application faster failure detection for individual frames without waiting for a peer-level timeout.

## Load Balancing

Today the system uses round-robin distribution across send sockets. The proposed change is pick-two: for each batch of frames destined for a peer, query two candidate paths (send sockets with their associated Peer Contexts) and select whichever will deliver bytes sooner.

The "deliver bytes sooner" heuristic considers the Peer Context's current queue depth, the congestion controller's available window, and potentially the next scheduled transmission time. The path that can put bytes on the wire sooner wins.

This naturally distributes load toward less-congested paths. If one path's Peer Context has a deep queue or a small congestion window, frames route to the alternative. This prevents any single Peer Context queue from growing disproportionately deep and provides a form of implicit backpressure distribution.

## Backpressure

The primary backpressure mechanism is the Writer's existing max_inflight_bytes. The Writer cannot submit more Frames until completions free up local budget. This bounds the total number of frames any single stream can have in the system (across the wheel, peer context queues, and in-flight on the wire).

Pick-two load balancing provides secondary pressure equalization across paths. By routing frames toward less-loaded Peer Contexts, it prevents queue depth asymmetry.

The congestion controller provides tertiary backpressure. The Peer Context only drains frames when the CCA permits transmission. If the network is congested, frames accumulate in the Peer Context queue, which eventually causes the pick-two heuristic to route elsewhere, which eventually causes all paths to fill, which eventually causes Writer completions to slow down, which eventually causes Writers to block on max_inflight_bytes. The backpressure propagates naturally.

## Future Work: Per-Peer Local Flow Control

There is a potential degenerate case: all streams target the same peer, the CCA is throttling, and Peer Context queues grow deep despite pick-two. The Writers' individual max_inflight_bytes limits don't aggregate into a per-peer limit, so the combined queue depth could be large (sum of all streams' inflight limits destined for one peer).

A future extension could add per-peer local flow control: limit total bytes submitted to any single Peer Context. When the limit is hit, the socket worker applies backpressure on the wheel, which propagates back to Writers. This would bound worst-case queue depth regardless of stream count.

This is documented as a future consideration. The current design relies on CCA backpressure and max_inflight_bytes being sufficient in practice, which should hold for the target workloads (many peers, not all streams hitting one destination). If production shows deep Peer Context queues, this extension provides the fix.

## Interaction with FlowInit and Sticky Routing

FlowInit, FlowInitValidate, and FlowValidateRequest frames can be aggregated with other frames in the same packet. Their deduplication semantics operate on the independent attempt_id field, not on packet numbers. A FlowInit frame packed alongside FlowData frames works fine — the receiver dispatches each frame's metadata to the appropriate handler regardless of what else is in the packet.

The stickiness constraint still applies during frame grouping. Frames with requires_sticky_sender_id must route to their designated sender socket. The instance pacer respects this during merge: sticky frames are submitted to the correct socket worker, and non-sticky frames can be freely distributed. This is the same constraint the batch builder enforces today, just applied at the frame-grouping stage instead of the batch-building stage.

## Performance Expectations

For the 500k small-stream workload at 1KB per RPC: instead of 500k packet numbers (one per RPC), we expect something like 50k-60k packets (packing 8-10 frames per MTU-sized segment at 1KB each). Per-packet costs — encryption, map insertion, ACK processing, CCA callbacks — all drop by a factor of 8-10x. ACK traffic drops proportionally.

For bulk transfers: a single stream's frames fill entire segments by themselves (one ~8KB payload per segment), so there is no aggregation overhead. The single-stream case should remain at parity with today's performance.

The sweet spot is exactly where the current architecture struggles: high-fan-out, small-payload workloads where per-packet overhead dominates.

## What Changes and What Stays

The Writer's responsibility narrows. It no longer builds GSO batches or worries about segment uniformity. It produces Frames and submits them. Flow control, completion handling, FIN semantics — all unchanged.

The Reader is unaffected in its reassembly logic. It still receives (stream_id, offset, payload) tuples. The difference is that these tuples arrive via a new dispatch path (iterating frame metadata within a packet) rather than today's one-packet-one-routing-info path.

The packet format changes. Today a packet contains one routing_info and one payload. The new format contains N frame metadata entries and N payloads. The header and payload length fields in the packet header tell the decoder how to partition these regions.

The Peer Context transforms from a passive data structure attached to batches into an active scheduler that owns a frame queue, self-registers in a local wheel, and performs packet assembly and encryption.

The PathResolver and Encoder pipeline stages are absorbed into the Peer Context. They no longer exist as separate components in the send pipeline.

The packet_number_map entries grow from single items to frame lists. ACK processing fans out to multiple completions per packet number.

The congestion controller interface is unchanged — it still sees bytes-on-wire per packet and gets called once per packet_sent and once per packet_acked.

## Implementation Strategy

This is a clean replacement of the send and receive paths. None of this code is deployed — the goal is to bring performance up before first deployment, so there is no backward compatibility concern. The old PartialDatagram-based pipeline can be ripped out entirely as each piece is replaced.

Implementation can proceed incrementally: first introduce the Frame struct and have Writers produce them, then modify the wheel to group by peer, then build the new Peer Context with its local wheel and packet assembly logic, then update the receiver to handle multi-frame packets. Each stage can be tested independently. The Writer API is unchanged (write_from, write_all_from_fin, shutdown) so application code doesn't need to change.
