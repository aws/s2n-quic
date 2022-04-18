# netbench-driver

This crate contains drivers for each transport protocol.

## Running driver tests

Netbench will have a dedicated CLI to automate running tests. Until then, the drivers can be used directly.

```sh
export DRIVER=s2n-quic
cargo build --release --bin netbench-driver-$DRIVER-server --bin netbench-driver-$DRIVER-client --bin netbench-scenarios
./target/release/netbench-scenarios
./target/release/netbench-driver-$DRIVER-server ./target/netbench/request_response.json
```

In another terminal

```sh
export DRIVER=s2n-quic
export SERVER_0=localhost:4433
./target/release/netbench-driver-$DRIVER-client ./target/netbench/request_response.json
```

> Note: if the netbench driver is being run on a mac, set the local IP on the client driver to 0.0.0.0 as follows: `--local-ip 0.0.0.0`

## Building docker images

```sh
sudo docker-compose --file netbench/netbench-driver/etc/docker-compose.yml --project-directory . build
```
