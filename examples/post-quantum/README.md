# post-quantum example

When using `s2n-tls` as the TLS provider, `s2n-quic` supports post-quantum key shares. However, because the key share algorithms are going through the standardization process, this functionality is disabled by default. It can be enabled by passing a compiler flag:

```sh
export RUSTFLAGS=`--cfg s2n_quic_enable_pq_tls`
```

You can also add a `.cargo/config.toml` to the project (as done in this example):

```toml
[build]
rustflags=['--cfg', 's2n_quic_enable_pq_tls']
```

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

