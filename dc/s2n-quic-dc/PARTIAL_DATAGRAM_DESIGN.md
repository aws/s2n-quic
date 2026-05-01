# Partial Datagram Design

## Overview

This document describes the design for partial datagram packets in the reliable datagram prototype. Applications generate partial packets that socket workers complete with encryption and packet numbering.

## Architecture

### Application Layer

Applications create `PartialDatagram` structures containing:
- **Routing Information**: Queue pair IDs or other routing metadata
- **Source Control Port**: Control port for the packet
- **Header**: Application-level header data (as `ByteVec`)
- **Payload**: Application payload data (as `ByteVec`)
- **Path Secret Entry**: Pre-computed `Arc<PathSecretEntry>` for the destination peer

### Socket Worker Layer

Socket workers receive partial datagrams and:
1. Derive or lookup peer-specific crypto state from the path secret entry
2. Assign the next packet number (used as nonce)
3. Encrypt the header+payload using the peer's current credentials
4. Send the completed datagram packet

## Key Components

### ByteVec

Imported from Membrain's `core-bytevec` implementation. A vectorized buffer supporting zero-copy operations:

```rust
pub struct ByteVec {
    len: usize,
    head: Bytes,
    additional: VecDeque<Bytes>,
}
```

**Benefits:**
- Avoids copying data when building multi-chunk payloads
- Efficient conversion to IoSlice arrays for vectorized I/O
- Implements `s2n_quic_core::buffer::{reader::Storage, writer::Storage}`

**Location:** `dc/s2n-quic-dc/src/byte_vec/`

### PartialDatagram

The core structure for application-generated packets:

```rust
pub struct PartialDatagram {
    pub routing_info: RoutingInfo,
    pub source_control_port: u16,
    pub header: ByteVec,
    pub payload: ByteVec,
    pub path_secret_entry: Arc<PathSecretEntry>,
}
```

**Location:** `dc/s2n-quic-dc/src/packet/datagram/partial.rs`

### RoutingInfo

Enum supporting different routing strategies:

```rust
pub enum RoutingInfo {
    None,
    QueuePair {
        source_queue_id: VarInt,
        dest_queue_id: VarInt,
    },
}
```

When `None`, packets are routed based on socket address only. When `QueuePair`, the queue IDs are encoded in the packet for multiplexing.

**Location:** `dc/s2n-quic-dc/src/packet/datagram.rs`

## Design Rationale

### Why Pre-compute Path Secret Entry?

Having the application pass the `Arc<PathSecretEntry>` avoids expensive map lookups on the socket worker hot path. The application already knows which peer it's sending to, so it can do the lookup once and include the result.

### Why Separate Workers Per Socket?

Each socket worker maintains its own packet number sequence and derived secrets. This avoids synchronization overhead between workers since:
- Packet numbers are used as nonces and must be unique per peer+key
- Each worker can maintain independent counters without coordination
- Workers can cache derived secrets independently

### Why ByteVec Instead of Single Buffer?

Applications may construct payloads from multiple sources (e.g., header + body from different allocations). ByteVec allows zero-copy aggregation of these chunks without allocating a contiguous buffer.

## Next Steps

1. **Worker Integration**: Implement the socket worker logic to consume `PartialDatagram` and produce encrypted packets
2. **Descriptor Pool**: Adapt the existing descriptor allocation to work with `ByteVec` payloads
3. **Completion Handling**: Design completion notifications back to the application layer
4. **Testing**: Create tests in `tools/wheel-demo` to exercise the full path

## Files Modified

- `dc/s2n-quic-dc/src/lib.rs` - Added `byte_vec` module
- `dc/s2n-quic-dc/src/packet/datagram.rs` - Added `partial` module and `RoutingInfo` enum
- `dc/s2n-quic-dc/src/packet/datagram/partial.rs` - New file with `PartialDatagram` and builder
- `dc/s2n-quic-dc/src/byte_vec/` - New directory with ByteVec implementation from Membrain

## API Example

```rust
use s2n_quic_dc::packet::datagram::{RoutingInfo, partial::PartialDatagram};
use s2n_quic_dc::byte_vec::ByteVec;
use bytes::Bytes;

// Application constructs a partial datagram
let mut payload = ByteVec::new();
payload.push_back(Bytes::from_static(b"Hello, world!"));

let partial = PartialDatagram::new(
    RoutingInfo::None,
    8080, // source control port
    ByteVec::new(), // empty header
    payload,
    path_secret_entry, // Arc<PathSecretEntry> from path secret map
);

// Submit to socket worker for encryption and transmission
socket_worker.send(partial).await?;
```
