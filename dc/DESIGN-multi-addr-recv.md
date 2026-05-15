# Multi-Address Recv Distribution

## Problem

Today, each peer advertises a single data port during the handshake. All send workers on the local side transmit to that single remote address, and the kernel's RSS (Receive Side Scaling) is responsible for distributing incoming packets across the peer's recv workers. This has two problems:

Linux RSS distribution depends on a Toeplitz hash over the 4-tuple. With a small number of ports, the hash produces significant imbalance — we've observed 1.7x skew between the busiest and lightest recv workers (137k vs 80k packets/sec). The busiest worker becomes a bottleneck that increases ACK processing latency, spuriously triggers PTO, and wastes bandwidth on retransmissions.

More fundamentally, RSS cannot handle multi-NIC topologies. If a host has two NICs, there's no way to steer traffic toward a specific NIC through kernel hashing alone. The sender is the only one with enough context to make this decision.

## Background

The current port exchange happens over a bidirectional QUIC stream during the TLS handshake. Each side sends 2 bytes (a `u16` port in big-endian). The result is stored on the path secret entry as a single port, and all send workers use `(peer_ip, peer_data_port)` as the destination address.

Send workers already have a stable identity (socket index 0..63). They already make per-peer, per-packet destination decisions at transmit time by looking up the path secret entry. The delivery-time feedback loop already adjusts pacing per destination path.

## Requirements

1. Recv workers must be individually addressable by remote senders, without relying on kernel RSS.
2. The mechanism must support multi-NIC topologies where recv workers bind to different IP addresses.
3. The distribution must be uniform under normal conditions, allowing the existing delivery-time load balancing to adjust for asymmetric path performance.
4. The design must not require connected sockets — send workers remain unconnected (they send to many peers).
5. The exchange format must be backwards-compatible or negotiable so that upgraded and non-upgraded peers can interoperate during rollout.
6. Per-packet send overhead must remain O(1) — no per-batch coordination or global state.

## Goals

- Eliminate recv worker imbalance caused by RSS hash collisions.
- Enable multi-NIC deployments without application-layer routing or eBPF programs.
- Keep the send path simple: a send worker looks up a single destination address for a given peer without additional synchronization.
- Minimize handshake overhead growth.

## Solution

### Recv Address List

Each recv worker binds to its own distinct socket address (IP + port). On startup, the endpoint collects the full list of recv addresses. This list is advertised to peers during the handshake.

### Exchange Format

Replace the current 2-byte port exchange with a length-prefixed list of socket addresses:

```
u8              address_count
[SocketAddr]    addresses (6 bytes each for IPv4, 18 bytes each for IPv6)
```

For 5 recv workers on a single IPv4 NIC, this is 1 + 5*6 = 31 bytes (up from 2 bytes). For dual-NIC IPv6 with 8 workers, it's 1 + 8*18 = 145 bytes. Both fit comfortably in a single QUIC stream frame.

For backwards compatibility: if the peer sends exactly 2 bytes, treat it as the legacy single-port format. If it sends a length-prefixed list, use the new format. The receiver can distinguish these by reading the first byte — a valid `address_count` will be small (1-32), while a big-endian port's high byte is typically non-zero for ephemeral ports (>256). If ambiguity is a concern, a version byte prefix (0x00 for legacy, 0x01 for multi-addr) can disambiguate cleanly.

### Send Worker Pinning

Each send worker (or send socket) has a stable index `i` in `0..S` where `S` is the local send socket count. For a given peer with `N` recv addresses, the send worker uses:

```
dest_addr = entry.recv_addrs[i % N]
```

This gives static, deterministic assignment with no runtime coordination. With 64 send sockets and 5 recv addresses, each recv worker gets ~13 senders. The distribution is perfectly uniform when `S % N == 0`, and off by at most 1 sender otherwise.

### Path Secret Entry Storage

The path secret entry grows to store the peer's recv address list:

```rust
struct RecvAddrs {
    addrs: [SocketAddr; MAX_RECV_WORKERS],
    count: u8,
}
```

`MAX_RECV_WORKERS` can be 32 (covers any realistic deployment). At 18 bytes per `SocketAddr` (IPv6 worst case) plus padding, this adds ~600 bytes to the entry. Given entries are long-lived and there's one per peer, this is acceptable.

### Delivery-Time Load Balancing

No changes needed. The existing delivery-time feedback loop operates per-path. If one recv address is backed by a slower NIC (or a congested path), senders assigned to that address will observe higher delivery times and reduce their rate. This provides organic, measurement-driven load balancing on top of the uniform static assignment.

## Recommendations

**Use full `SocketAddr` rather than just ports** (satisfies requirements 1, 2). This handles multi-NIC directly and is future-proof for topologies where recv workers span subnets.

**Static index-based pinning** (satisfies requirements 3, 4, 6). Each send worker computes its destination with a single modulo operation. No locking, no cross-worker coordination, no connected sockets. The assignment is fixed for the lifetime of the path secret entry.

**Length-prefixed exchange with legacy fallback** (satisfies requirement 5). The format change is minimal, easily detected, and gracefully degrades to single-port behavior with older peers.

**Leverage existing delivery-time feedback** (satisfies requirement 3). Uniform assignment is the default. Asymmetric path performance is handled by an existing, proven mechanism without adding any new balancing logic.
