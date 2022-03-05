# netbench-driver

This crate contains drivers for each transport protocol.

## Running driver tests

Netbench will have a dedicated CLI to automate running tests. Until then, the drivers can be used directly.

```
export DRIVER=s2n-quic
cargo build --release --bin netbench-driver-$DRIVER-server --bin netbench-driver-$DRIVER-client --bin netbench-scenarios
./target/release/netbench-scenarios
./target/release/netbench-driver-$DRIVER-server ./target/netbench/request_response.json
```

In another terminal

```
export DRIVER=s2n-quic
export SERVER_0=localhost:4433
./target/release/netbench-driver-$DRIVER-client ./target/netbench/request_response.json
```
