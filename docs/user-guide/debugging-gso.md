## GSO and GRO

Generic Segmentation Offload (GSO) and Generic Receive Offload (GRO) are network stack features that can improve the efficiency of transmitting and receiving packets. GSO/GRO are enabled by default in `s2n-quic` on operating systems that have good support for these features (namely Linux variants). `s2n-quic` will also automatically disable these features if the operating system emits a socket error indicating they are not supported. 

Some operating systems may silently fail when using GSO/GRO, leading to performance degradation. In these cases, `s2n-quic` will not automatically disable GSO/GRO. You can manually disable GSO/GRO on the IO provider to evaluate if that has a positive impact on performance:

```rust
let io = s2n_quic::provider::io::Default::builder()
    .with_gso(false)? // disable GSO
    .with_gro(false)? // disable GRO
    .build()?;

let mut server = Server::builder()
    .with_io(io)?
    .start()?;
```