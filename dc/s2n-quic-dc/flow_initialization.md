# Flow Initialization Protocol Design

## Overview

This protocol establishes bidirectional communication flows between client and server using queue-based routing. Each flow represents an independent stream of data with its own ordering and flow control. The design prioritizes zero-RTT establishment in the common case while providing stateless retry mechanisms for edge cases.

## Design Goals

**Zero-RTT establishment:** Clients send application data in the first packet, eliminating handshake overhead when server state is available.

**Stateless operation under pressure:** Servers can challenge clients when deduplication state is unable to guarantee replay protection without maintaining per-flow state during the challenge period.

**Efficient deduplication:** Stream identifiers prevent duplicate flow creation during retransmission without requiring time-based state expiration.

**Backpressure throughout:** The protocol provides flow control at every stage to prevent resource exhaustion from unaccepted flows.

**Integration with packet layer:** Flow-level deduplication integrates with path contexts to minimize allocations and lookups.

## Packet Types

### FlowInit

Sent by the client to initiate a flow with early data. Contains the first application datagram along with routing and identification metadata.

**Fields:**

- source_sender_id: Identifies the client socket for routing control packets back
- source_queue_id: The client's queue identifier for routing future data and control packets
- dest_acceptor_id: Application-level service identifier determining which handler processes this flow
- attempt_id: Monotonically increasing identifier for deduplication, scoped to the sending path context (sender-local)
- stream_id: Global client-wide identifier for flow validation

**Semantics:**
The attempt_id must increase monotonically within a given credentials and sender pair. This enables sliding window deduplication on the receive side. The attempt_id is sender-local and used purely for detecting duplicate FlowInit packets during retransmission.

The stream_id is a global client-wide identifier (e.g., AtomicU64 counter) that uniquely identifies this flow. Once a flow is established with a given (credential_id, stream_id) pair, all subsequent packets for that flow must present the same pair for validation. If validation fails, the server sends FlowReset.

The early data payload is limited to approximately MTU minus headers to guarantee single-packet transmission.

### FlowRetryRequest

Sent by the server when it cannot guarantee deduplication (attempt_id outside sliding window).

**Fields:**

- source_sender_id: Server's sender identifier for routing the retry response
- queue_pair: Contains source_queue_id (server's optimistically allocated queue ID for this flow) and dest_queue_id (client's queue identifier from the original FlowInit)
- attempt_id: Echoed from FlowInit for client validation
- stream_id: Echoed from FlowInit for client validation

**Semantics:**
The client must validate that both the echoed attempt_id and stream_id match what it sent. It must also ensure the state associated with the local queue_id matches the credential_id and stream_id. If any of these checks fail, it must send a FlowReset to the server so it can release the queue_id back to its pool. If all checks pass, then a FlowInitRetry is sent.

### FlowInitRetry

Sent by the client in response to FlowRetryRequest, proving that the flow creation attempt is current and not replayed.

**Fields:**

- source_sender_id: Client's sender identifier
- queue_pair: Contains source_queue_id (client's queue identifier from original FlowInit) and dest_queue_id (server's queue identifier from FlowRetryRequest)
- attempt_id: Same as original FlowInit
- stream_id: Same as original FlowInit

**Semantics:**
This packet must contain the same early data payload as the original FlowInit. The server performs validation - the credential_id and stream_id associated with the local queue_id must match. If this validation fails, a FlowReset is sent. If it passes, the stream is enqueued in the acceptor queue.

The attempt_id in FlowInitRetry proves freshness (it's responding to a recent challenge), while the stream_id establishes the global flow identity.

### FlowControl

Sent by a receiver to provide flow control window updates or to indicate receiver cancellation.

**Fields:**

- source_sender_id: Sender identifier transmitting the packet packet
- queue_pair: Contains both source_queue_id and dest_queue_id
- stream_id: Identifies the stream

**Payload:**
The control data contains MAX_DATA frames indicating how much data the sender may send. On the server, this window opens after the application has actually accepted the stream in userspace. An optimization may include allocating some additional window while it's waiting in the accept queue, provided the server is not overloaded.

**Semantics:**
Receipt of FlowControl transitions the client to ESTABLISHED state. The client may now send additional data up to the advertised window. This packet implicitly acknowledges the FlowInit or FlowInitRetry that created the flow.

### FlowData

Standard data packet for an established flow, routed via queue pair.

**Fields:**

- source_sender_id: Sender's socket identifier for routing ACKs
- queue_pair: Both source and destination queue identifiers for bidirectional routing
- stream_id: Identifies the stream

**Semantics:**
Once a flow is established, all data uses this packet type. The queue_pair enables efficient routing on both sides. These packets participate in normal reliability and congestion control provided by the reliable datagram layer.

### FlowReset

Sent by an endpoint when no stream state is associated with the local queue. This can happen in several scenarios.

**Fields:**

- source_sender_id: Sender identifier transmitting the packet
- queue_pair: Contains source_queue_id and dest_queue_id for bidirectional routing
- stream_id: Identifies which stream is being rejected
- error_code: Reason for rejection

**Error codes:**

- ACCEPTOR_NOT_FOUND: The dest_acceptor_id does not correspond to any running service
- ACCEPT_QUEUE_OVERFLOW: The acceptor queue has overflowed and is rejecting requests.
- STALE_STATE: The credentials and stream_id associated with the queue_id are out of date.

**Semantics:**
The original sender receives this error, looks up the pending flow by queue and stream identifiers, and delivers the error to the application without retrying. The queue_id may be released immediately for reuse.

## Deduplication Strategy

### Sliding Window per Path Context

Each path context maintains a sliding window of recently seen attempt identifiers. This integrates with the existing path context structure in the pipeline to minimize lookups. When a FlowInit arrives, the pipeline checks whether the attempt_id falls within the window and whether it has been previously processed.

The window tracks the highest observed attempt_id and maintains a bitmap or interval set of seen identifiers within a fixed range below that maximum. The window size should accommodate realistic reordering depths—perhaps one thousand to four thousand attempts. Given mostly-FIFO sender behavior, this captures all legitimate retransmissions without time-based expiration.

**Inside window, previously seen:** Drop the packet silently as a duplicate. The flow already exists or has already been created.

**Inside window, not seen:** Mark the attempt_id as seen, proceed with flow creation. Check the global flow tracker (DashMap keyed by stream_id) to ensure this stream_id hasn't been used with different credentials.

**Outside window (too old):** The attempt_id is far below the current window. The server cannot guarantee this is not a duplicate from long ago. Check the global flow tracker - if the stream_id exists, this is a retransmission (ACK the packet). If not found, send FlowRetryRequest to obtain a fresh proof.

**Memory management:** Each path context allocates a bounded structure for the dedup window. Under memory pressure, evict entire path contexts based on idle time, which cascades to evicting their dedup state. Subsequent packets from evicted contexts trigger FlowRetryRequest.

### Challenge-Response for Stateless Operation

When the server cannot guarantee deduplication (either because the stream_id is outside the window or the path context was evicted) it uses challenge-response to establish freshness without maintaining state. This ensures both sides agree on the current associated state of their queues. If the state has changed, then the flow is reset. Note that peers have already established trust with the mTLS handshake that established credentials. So something like a cryptographic retry token mechanism is overkill.

## Server State Transitions

### Receive Path Integration

When a FlowInit packet arrives, it first passes through standard packet-level deduplication and decryption in the packet pipeline. After authentication, the pipeline extracts the routing_info and inspects the packet type.

For FlowInit, the pipeline performs stream-level deduplication by consulting the path context's sliding window. If the stream passes deduplication, the pipeline determines which acceptor queue should receive it based on dest_acceptor_id.

### PENDING State Before Acceptance

After successful deduplication, the flow enters a PENDING state. The server allocates minimal state tracking the stream_id and enqueues a descriptor to the acceptor. Crucially, the local receiver flow control window is not yet opened.

This PENDING state prevents flooding. If the acceptor queue has bounded depth, new flow requests apply backpressure by slowing down the dequeue from the receive path. The client's retransmission timers naturally slow down as ACKs arrive for the FlowInit packet itself—the packet was received and ACKed at the transport layer even though the flow has not yet been accepted at the application layer.

The PENDING state tracks a small initial credit—perhaps the size of the early data already received. The server ACKs the FlowInit packet but does not send FlowControl until the acceptor explicitly accepts the flow.

### Acceptor Processing

The acceptor dequeues flow requests from its queue and decides whether to accept or reject each one. If accepting, the acceptor calls an API that transitions the flow to ESTABLISHED and sends FlowControl with an initial window. If rejecting, the acceptor specifies an error code and the server sends FlowReset.

The acceptor queue provides natural backpressure. When the queue depth exceeds a threshold, the server can begin sending FlowReset with QUEUE_OVERLOADED before even enqueueing to the acceptor. This sheds load early.

### Load Shedding

Under extreme load, the server can adopt increasingly aggressive policies:

**Moderate load:** Queue depth high but manageable. Continue accepting flows but send smaller initial windows in FlowControl.

**Heavy load:** Queue approaching capacity. Begin rejecting some percentage of flows with QUEUE_OVERLOADED. Prefer rejecting flows from clients with many concurrent pending flows.

**Severe load:** Queue at capacity. Reject all new flows immediately with QUEUE_OVERLOADED without enqueueing to acceptor.

The server can also trigger FlowRetryRequest more aggressively under load, even for stream_ids within the dedup window. This adds one RTT but allows the server to apply temporal filtering—if the client retries quickly, the server knows the request is urgent.

## Client State Management

### Application-Level State Machine

The client's flow state machine lives in application space rather than in the pipeline. The application initiates a flow by constructing a FlowInit packet with early data and calling a send API. The underlying reliable datagram layer handles packet-level retransmissions, ACK processing, and completion notifications.

From the application's perspective, the flow transitions through these states:

**IDLE:** No flow exists.

**INIT_SENT:** FlowInit sent, waiting for acceptance signal. The application may queue additional data locally but cannot send it until the window opens.

**RETRY_SENT:** FlowRetryRequest received, FlowInitRetry sent, waiting for acceptance signal.

**ESTABLISHED:** FlowControl or FlowData received, window is open, application may send data.

**CLOSED:** FlowReset received or local close initiated.

The application does not manually retransmit FlowInit or FlowInitRetry packets—the reliable datagram layer handles this. The application simply waits for state transitions triggered by incoming packets.

### Handshake Completion Signal

When the application receives FlowControl or FlowData, it drops its closes its handshake completion queue. This informs the datagram layer to avoid retransmission of FlowInit and FlowInitRetry packets for this stream, even if packet-level ACKs have not arrived for all of them.

This mechanism is separate from packet-level ACKs. A FlowInit packet might be ACKed at the packet layer but still not accepted by the application acceptor. Conversely, receiving FlowControl stops retransmitting FlowInit even if the packet-level ACK has not yet arrived.

## Flow Control and Backpressure

### Initial Window Strategy

When the server sends FlowControl, it must choose an initial window size that balances throughput and memory capacity. A too-small window adds round trips and limits bandwidth; a too-large window risks memory exhaustion if many flows are accepted but not actively read by the application.

The initial window could scale based on acceptor queue depth. When the queue is shallow, provide a generous window. When the queue is deep, provide a minimal window forcing the client to wait for additional credit.

The server might also distinguish between acceptors. A latency-sensitive acceptor could receive larger initial windows, while a background processing acceptor receives smaller windows.

### Per-Flow Credit Management

Once established, each flow independently manages its send and receive windows using standard flow control frames. The server sends MAX_DATA to open the receive window; the client sends MAX_DATA to open its receive window.

The flow can dynamically adjust credit based on application read rate. If the application reads quickly, the flow grants more credit. If the application stalls, the flow stops granting credit, which propagates backpressure to the peer. Flows should also limit the local inflight packets to reduce memory pressure.

### Congestion Control Interaction

Flow control operates above congestion control. Congestion control limits how fast packets can be sent based on network conditions. Flow control limits how much data can be outstanding based on buffer capacity.

A flow might have available flow control window but be blocked by congestion control, or vice versa. The sender transmits at the minimum of both constraints.

## Edge Cases and Failure Modes

### Acceptor Queue Overflow

If the acceptor queue reaches capacity and a new FlowInit arrives, the server sends FlowReset with QUEUE_OVERFLOWED immediately. The client should not retry immediately—this would worsen the overload. Instead, the client applies exponential backoff before retrying the flow creation.

The application might also surface this error to the user or fail over to a different server.

### ID Wraparound

Both attempt_id and stream_id are VarInt values, providing up to sixty-two bits. At one million flows per second, wraparound takes five hundred thousand years. Wraparound is not a practical concern.

If wraparound somehow occurs for attempt_id, the sliding window logic continues to function correctly as long as the window size is much smaller than the VarInt space. The window tracks relative positions, not absolute values.

For stream_id wraparound, the global tracker (DashMap) would need to handle collisions, but this is not a practical concern given the 62-bit space.

### Server Restart

When the server restarts, all path contexts and dedup state are lost. The first packet from a client with old credentials triggers UnknownPathSecret. The client receives this, closes all flows associated with those credentials, and re-establishes the path secret through the credential negotiation protocol.

Flow initialization then proceeds normally using new credentials.

### Client Address Change

This protocol targets datacenter environments with stable IP addresses. Address migration is explicitly not supported. If a client's IP address changes, the path secret becomes invalid, triggering UnknownPathSecret and forcing credential renegotiation.

## Graceful Closure

### Application-Driven Shutdown

When the application finishes sending data on a flow, it waits for completion notifications from the reliable datagram layer indicating all packets have been ACKed. Once complete, the application releases the queue_id back to the available pool.

No explicit FIN or close packet is required. The queue_id simply becomes available for future flows.

### Idle Timeout

Both sides maintain idle timers for path contexts in the packet processing pipeline. If no packets are sent or received on a path context for the timeout period, the state is released automatically. This handles cases where one side crashes or loses state without sending explicit shutdown signals. If packets are being transmitted but not being ACKed, the PTO period backs off until the idle timeout is exceeded, at which point all outstanding datagrams are completed with an error and the application is notified of the error.

## Open Questions and Future Considerations

### Multi-Path Support

Each flow is bound to a single path context. Supporting multi-path transmission within a flow would require coordination across path contexts and is out of scope for this design. For now, flow initialization is pinned to a sender_id for simplicity in deduplicating. This should be fine since it's only a handful of packets. Once the flow is established, the full multi-path routes can be utilized by the flow.
