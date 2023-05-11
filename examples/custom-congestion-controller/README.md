# Custom Congestion Controller

This folder contains an example of implementing and configuring a custom congestion controller in `s2n-quic`. `s2n-quic` includes [CUBIC](https://www.rfc-editor.org/rfc/rfc8312) and [BBRv2](https://datatracker.ietf.org/doc/html/draft-cardwell-iccrg-bbr-congestion-control) congestion controller implementations, but you may
 implement the `CongestionController` trait, found in [congestion_controller.rs](../../quic/s2n-quic-core/src/recovery/congestion_controller.rs), to provide your own.

# Set-up

The `CongestionController` trait is considered unstable and may be subject to change in a future release. In order to build it you must pass a compiler flag:
```sh
export RUSTFLAGS="--cfg s2n_quic_unstable"
```
and add this line to your Cargo.toml file:
```toml
[dependencies]
s2n-quic = { version = "1", features = ["unstable-congestion-controller"]}
```
