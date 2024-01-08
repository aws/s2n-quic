# Session Resumption

This folder contains example code to do resumption handshakes in s2n-quic. This TLS feature allows a client and server to skip the costly certificate authentication step using a saved value from their previous TLS session. Note that currently resumption has only been implemented for the s2n-tls provider and therefore you will have to use s2n-tls rather than rustls to turn on this feature in s2n-quic.

# Set-up

Currently resumption is disabled by default as it is still in development. It can be enabled by passing a compiler flag:
```sh
export RUSTFLAGS="--cfg s2n_quic_unstable"
```
and adding this line to your Cargo.toml file:
```toml
[dependencies]
s2n-quic = { version = "1", features = ["unstable-resumption"]}
```
