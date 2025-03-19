## Packet capture

A packet capture allows for inspecting the contents of every packet transmitted or received by `s2n-quic`. Along with tracing logs, this can be very helpful for diagnosing issues. Follow these steps to record a packet capture.

### 1. Enable key logging on the TLS provider

Since QUIC is an encrypted transport protocol, the payload of each packet is not readable in a standard packet capture. `s2n-quic` supports exporting the TLS session keys used by each QUIC connection so that the packet capture may be decrypted. Both the `s2n-tls` and `rustls` TLS providers support key logging through their associated builders:

```rust
let tls = s2n_quic::provider::tls::default::Server::builder()
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