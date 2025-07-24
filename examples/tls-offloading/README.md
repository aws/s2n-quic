# TLS offload

s2n-quic is single-threaded by default. This can cause performance issues in the instance where many clients try to connect to a single s2n-quic server at the same time. Each incoming Client Hello will cause the s2n-quic server event loop to be blocked while the TLS provider completes the expensive cryptographic operations necessary to process the Client Hello. This has the potential to slow down all existing connections in favor of new ones. The TLS offloading feature attempts to alleviate this problem by moving each TLS connection to a separate async task, which can then be spawned by the runtime the user provides.

To do this, implement the `offload::Executor` trait with the runtime of your choice. In this example, we use the `tokio::spawn` function as our executor: 
```
struct TokioExecutor;
impl Executor for TokioExecutor {
    fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
        tokio::spawn(task);
    }
}
```

# Warning
The default offloading feature as-is may result in packet loss in the handshake, depending on how packets arrive to the offload endpoint. This packet loss will slow down the handshake, as QUIC has to detect the loss and the peer has to resend the lost packets. We have an upcoming feature that will combat this packet loss and will probably be required to achieve the fastest handshakes possible with s2n-quic: https://github.com/aws/s2n-quic/pull/2668. However, it is still in the PR process.

# Set-up

Currently offloading is disabled by default as it is still in development. It can be enabled by adding this line to your Cargo.toml file:

```toml
[dependencies]
s2n-quic = { version = "1", features = ["unstable-offload-tls"]}
```
