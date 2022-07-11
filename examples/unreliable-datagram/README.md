# Unreliable Datagram

This folder contains an example of sending and receiving an 'unreliable datagram' in `s2n-quic`. An 'unreliable datagram' is data that is not re-transmitted upon packet loss, and is defined in https://datatracker.ietf.org/doc/rfc9221. This is in contrast to 'stream data' which is re-transmitted during packet loss. Stream data is the preferred method of sending application data, as it comes with support for features like congestion-controllers and packet-retransmission. Additionally, unreliable datagram users are responsible for making sure their datagrams fit into the packet space given, as the RFC does not support data re-assembly. A common use-case for unreliable datagrams is video or audio streaming, where individual datagrams are small and some loss is acceptable.

# Default Datagram Implementation

`s2n-quic` contains a default implementation of the unreliable datagram sender and receiver. This implementation is minimal, given the many different ways that unreliable datagrams could be used. Users should look out for the fact that datagrams that are too large to be sent will be automatically dropped, and old datagrams will be dropped in favor of new datagrams from the peer if there is no more space on the receiving queue.

# Set-up

Currently the unreliable datagram functionality is disabled by default as it is still in development. It can be enabled by passing a few compiler flags:

```sh
export RUSTFLAGS=`--cfg s2n_quic_unstable --cfg unstable-provider-datagram`
```

You can also add a `.cargo/config.toml` to the project (as done in this example):

```toml
[build]
rustflags=['--cfg', 's2n_quic_unstable', '--cfg', 'unstable-provider-datagram']
```

## Running the Example

Spin up a QUIC server that sends an unreliable datagram:

```sh
cargo run --bin datagram_sender
```

and in another shell, the client that receives that datagram:

```sh
cargo run --bin datagram_receiver
```

The receiver will print out some bytes that the server sent.

# Custom Datagram Implementation

Users can implement their own datagram sender and receiver if the default implementation doesn't work for their use-case. To do this implement the datagram Endpoint trait, and the datagram Sender and Receiver traits, found in [traits.rs](../../quic/s2n-quic-core/src/datagram/traits.rs).
