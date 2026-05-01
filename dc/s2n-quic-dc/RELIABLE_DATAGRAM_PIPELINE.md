# Reliable Datagram Pipeline Design

## Architecture

```
Application Thread(s)
    ↓ [intrusive_queue::sync]
Instance-Wide Wheel (sorted by tx_time)
    ↓ [Paced + Flatten combinators]
Dispatcher (round-robin → TODO: load balancer)
    ↓ [to socket workers]
Socket Worker [encoder task + ACK receiver task]
    ↓ [GSO batch transmission]
Network
    ↓ [ACKs return on same 5-tuple]
Socket Worker ACK Receiver
    ↓ [intrusive_queue for completions]
Application Thread
```

## Key Decisions

### Batch Structure
- **Type**: `DatagramBatch` containing `Queue<PartialDatagram>`
- **Implements**: `Scheduled` trait for wheel compatibility
- **Metadata**: tx_time (1us intervals for each sender), total_bytes, peer_addr
- **Completion**: `Weak<dyn BatchNotify>`

### Application Thread
- Query `gso.max_segments().min(msg::segment::MAX_COUNT)` for batch size limit
- Build batches as intrusive queues (all datagrams to same peer)
- Assign tx_time at 1us intervals to force interleaving
- Submit to wheel via `intrusive_queue::sync::Sender`

### Wheel & Dispatch
- Reuse existing `Wheel<DatagramBatch, _, _, 1>` from wheel-demo
- Channel combinators: `Flatten` → `Paced` → `Dispatcher`
- Dispatcher: round-robin for Phase 1, load balancer combinator for Phase 2

### Socket Worker (per-worker state)
- **Encoder task**: Receives batches, encodes with encryption, transmits GSO batch
- **ACK receiver task**: Listens on same socket for ACKs on same 5-tuple. Needs to detect loss with the standard QUIC loss detection algorithm with both loss thresholds and time.
- **Secret cache**: Per-worker HashMap<Arc<PathSecretEntry>, DerivedSecret> (no sync needed)
  - We need to figure out clean up when these path secrets go away. It might be better to
    actually store the state on the PathSecretEntry? And then each socket worker could put its own information in there around cached secrets, CCA, RTT, or current inflight packets.
- **Packet number counter**: Per-worker monotonic counter (nonces for encryption)
- **Packet number map**: `s2n_quic_core::packet_number::Map<Packet>` for ACK tracking
- **PTO/Recovery**: We also need to have the worker send PTO packets if any datagrams are in flight and the PTO timer fires. This ensures recovery kicks in.

### Packet Number Map
- **Key**: PacketNumber (per-worker monotonic)
- **Value**: `Packet { partial_datagram, completion, }`
- **Insert**: On packet transmission
- **Remove**: On ACK received
- **Completion**: Notifies the application that the packet has been received.

### ACK Receiver
- Runs as separate task on same socket as encoder
- Receives ACKs coming back on same 5-tuple (fast worker-local response)
- Decodes ACK ranges, updates packet number map
- Sends completion notifications via intrusive_queue channel

### Completion Notification
- **Channel**: `intrusive_queue::sync` from worker to application
- **Types**: `Acknowledged { packet }` or `Failed { reason, packet }`
- **Reasons**:
  - PeerDead - PTO + max idle
  - TransmissionError - the peer is still active but refused to accept the packet
  - UnknownPathSecret - the path secret entry provided has been refused by the peer and the flow has been reset.

## Implementation Phases

### Phase 1: Basic Pipeline
- Define `DatagramBatch` with `Scheduled` trait
- Application batch submission API
- Socket worker encoder (no ACK handling yet)
- Use existing wheel + round-robin

### Phase 2: ACK Handling
- ACK receiver task per worker
- `packet_number::Map` integration
- Completion notifications

### Phase 3: Load Balancer Combinator
- Track per-worker metrics (cwnd, bandwidth, bytes_in_flight, backlog)
- Channel combinator for intelligent dispatch
- Per-peer load balancing decisions

### Phase 4: Receiver Side
- Datagram receiver
- ACK generation on same 5-tuple
- Duplicate detection

## Open Questions
1. ACK packet format for reliable datagrams?
2. Timeout/retransmission policy?
3. Congestion control integration?