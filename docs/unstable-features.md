# Unstable Features

s2n-quic has a few unstable features which are either experimental or under development. These
features can be enabled at compile time but do not have the same stability guarantees as the
 default s2n-quic API.

 To enable unstable features s2n-quic must be compiled with RUSTFLAGS="--cfg=s2n_quic_unstable".

### unstable_client_hello
todo

### unstable-provider-datagram
todo

### unstable-provider-io-testing
todo

### unstable-provider-packet-interceptor
todo

### unstable-provider-random
todo

 ### eBPF Event provider [unstable-provider-event-bpf]
 *This feature is unstable until MSRV is `1.59` or above.*

```
// compile s2n-quic with bpf enabled
RUSTFLAGS="--cfg=s2n_quic_unstable --cfg=unstable_provider_event_bpf" cargo build --release

// launch s2n-quic-qns process with bpf probes enabled
sudo bpftrace -c './target/release/s2n-quic-qns interop server --port 4433' quic/s2n-quic-core/src/event/s2n_quic.bt
```

The above command launches the s2n-quic-qns process with the bpf events enabled. BPF events mirror
events on the [Subscriber](https://docs.rs/s2n-quic/latest/s2n_quic/provider/event/trait.Subscriber.html).

Events are being added and so not all events provide the same level of details. By default the event do not
contain the same detail of information as the other providers.

**Event information:**
- [header file](/quic/s2n-quic-core/src/event/generated_s2n_quic_bpf_events.h) lists events which expose
detailed information.
- `connection level events`: by default `arg0` is the connection unique internal connection `id`
- `endpoint/platform level events`: do not have additional information

