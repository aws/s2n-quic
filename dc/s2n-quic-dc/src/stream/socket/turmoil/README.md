# Turmoil Integration for s2n-quic-dc

This module provides [turmoil](https://docs.rs/turmoil) network simulation support for s2n-quic-dc streams, enabling deterministic testing of distributed systems.

## Features

- Enables deterministic network simulation testing
- Supports network partitioning and repair for fault injection
- Works with turmoil's simulated time for reproducible tests

## Usage

Add the `unstable-provider-io-turmoil` feature to your `Cargo.toml`:

```toml
[dev-dependencies]
s2n-quic-dc = { version = "0.72", features = ["unstable-provider-io-turmoil"] }
turmoil = "0.7"
```

## Limitations

- Turmoil doesn't support vectored I/O, so only the first buffer in scatter/gather operations is used
- ECN (Explicit Congestion Notification) is not simulated by turmoil
- GSO (Generic Segmentation Offload) is not supported

## See Also

- [turmoil documentation](https://docs.rs/turmoil)
