# post-quantum example

When using `s2n-tls` as the TLS provider, `s2n-quic` supports post-quantum key
shares in the default configuration loaders. Currently s2n-quic's providers
default to the s2n-tls `default_pq` policy, although this is not stable and may
change in the future. See the [s2n-tls documentation](https://aws.github.io/s2n-tls/usage-guide/ch16-post-quantum.html)
for more details.

When using `rustls` or a custom TLS provider, s2n-quic will use their
configuration for post-quantum defaults.

## Running the example

Now we can spin up a pq-enabled QUIC server:

```sh
cargo run --bin pq_server
```

and in another shell, the client:

```sh
cargo run --bin pq_client
```

Inspecting traffic with wireshark will show the `key_share` extension with `Group: Unknown (12089)` in both the Client Hello and Server Hello.
