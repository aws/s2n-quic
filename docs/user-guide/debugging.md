# Debugging s2n-quic

Before [opening an issue](https://github.com/aws/s2n-quic/issues/new?template=s2n-quic-issue.md) regarding `s2n-quic`, you may be able to solve the issue on your own (or at least collect useful debugging information) by performing the actions contained within this section. Attaching the output from these actions to the issue increases our ability to address your issue in a timely manner.

## Tracing logs

`s2n-quic` includes an Event framework that emits debugging information everytime a connection is started, a packet is received, a datagram is dropped, and [many other situations](https://docs.rs/s2n-quic/latest/s2n_quic/provider/event/trait.Subscriber.html#provided-methods). When the `provider-event-tracing` feature is enabled, the default behavior of `s2n-quic` is to emit these events via [tracing](https://docs.rs/tracing). Configuring a `tracing-subscriber` will allow for the events to be emitted to a log file or stdout. Follow these steps to emit a tracing log to stdout:

### 1. Enable the `provider-event-tracing` feature
This feature is not enabled by default in `s2n-quic`, so specify it in your `Cargo.toml` in the `s2n-quic` dependency:
```toml
[dependencies]
s2n-quic = { version = "1", features = ["provider-event-tracing"]}
```

### 2. Add a dependency on `tracing-subscriber`
`tracing-subscriber` is used for collecting the event data emitted by `s2n-quic` and outputting it to stdout. The `env-filter` feature is used for turning logging off and on based on the `RUST_LOG` environment variable.
 ```toml
[dependencies]
s2n-quic = { version = "1", features = ["provider-event-tracing"]}
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### 3. Configure and initialize a global `tracing-subscriber`
In your application code, prior to starting an `s2n-quic` server or client, include the following code to initialize a global `tracing-subscriber`.  This configuration allows for the `RUST_LOG` environment variable to determine the logging level.

```rust
let format = tracing_subscriber::fmt::format()
    .with_level(false) // don't include levels in formatted output
    .with_timer(tracing_subscriber::fmt::time::uptime())
    .with_ansi(false) 
    .compact(); // Use a less verbose output format.

tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .event_format(format)
    .init();
```

### 4. [Optional] Specify the  `tracing::Subscriber` Event provider to `s2n-quic` server or client
When the `provider-event-tracing` feature is enabled, the default behavior of `s2n-quic` is to emit these events via [tracing](https://docs.rs/tracing). If your application already makes use of a custom event subscriber, you may need to explicitly specify the default `event::tracing::Subscriber` by composing it with your existing event subscriber (`MyCustomEventSubscriber` in this example):

```rust
let mut server = Server::builder()
   .with_tls((CERT_PEM, KEY_PEM))?
   .with_io("127.0.0.1:4433")?
   .with_event((
       MyCustomEventSubscriber,
       s2n_quic::provider::event::tracing::Subscriber::default(),
    ))?
   .start()?;
```

### 5. Run your application with the `RUST_LOG` environment variable
Now that everything has been configured, you can set the `RUST_LOG` environment variable to `debug` to start emitting debugging information:
```bash
 $ RUST_LOG=debug cargo run --bin my_application
 0.032760542s s2n_quic:server: platform_feature_configured: configuration=Gso { max_segments: 1 }
 0.032954042s s2n_quic:server: platform_feature_configured: configuration=BaseMtu { mtu: 1228 }
 0.032964625s s2n_quic:server: platform_feature_configured: configuration=InitialMtu { mtu: 1228 }
 0.032971583s s2n_quic:server: platform_feature_configured: configuration=MaxMtu { mtu: 1228 }
 0.032978167s s2n_quic:server: platform_feature_configured: configuration=Gro { enabled: false }
 0.032987833s s2n_quic:server: platform_feature_configured: configuration=Ecn { enabled: true }
 0.033881250s s2n_quic:server: platform_event_loop_started: local_address=127.0.0.1:4433
...
```
Capture this output and attach it to your issue to aid with debugging.

## Packet capture

A packet capture allows for inspecting the contents of every packet transmitted or received by `s2n-quic`. Along with tracing logs, this can be very helpful for diagnosing issues. Follow these steps to record a packet capture.

### 1. Enable key logging on the TLS provider

Since QUIC is an encrypted transport protocol, the payload of each packet is not readable in a standard packet capture. `s2n-quic` supports exporting the TLS session keys used by each QUIC connection so that the packet capture may be decrypted. Both the `s2n-tls` and `rustls` TLS providers support key logging through their associated builders:

```rust
let tls = s2n_quic::provider::tls::default::Serverhell::builder()
    .with_certificate(CERT_PEM, KEY_PEM)?
    .with_key_logging()?  // enables key logging
    .build()?;

let mut server = Server::builder()
   .with_tls(tls)?
   .with_io("127.0.0.1:4433")?
   .start()?;
```

#### 2. Start capturing packets

Popular tools for capture packets include the command line tools [tcpdump](https://www.tcpdump.org/) and [tshark](https://www.wireshark.org/docs/man-pages/tshark.html), as well as [Wireshark](https://www.wireshark.org/). Determine the network interface you are using for communicating with `s2n-quic` and provide it to the packet capture tool you prefer. The following example uses `tcpdump` to capture on the loopback interface and write the capture to a file: 

```bash
$ sudo tcpdump -i lo0 -w /var/tmp/mycapture.pcap
```

#### 3. Run your application with the `SSLKEYLOGFILE` environment variable

Set the `SSLKEYLOGFILE` environment variable to a file path to create a file containing the TLS session keys:

```bash
$ SSLKEYLOGFILE=/var/tmp/keys.log cargo run --bin my_application
```

#### 4. [Optional] Embed the key log in the packet capture file

To simplify analysis of the packet capture, it can be helpful to embed the key log from the previous step into the packet capture file itself. [editcap](https://www.wireshark.org/docs/man-pages/editcap.html) is a utility for editing packet captures and can perform this embedding: 

```bash
$ editcap --inject-secrets tls,/var/tmp/keys.log /var/tmp/mycapture.pcap /var/tmp/capturewithkeys.pcapng
```

Attach `capturewithkeys.pcapng` to your issue to aid with debugging. If you skipped step 4, also attach `keys.log`. 